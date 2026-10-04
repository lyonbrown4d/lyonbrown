import { deriveScope, type ScopeIdentity } from "@lyonbrown/local-control-plane";
import { validateOptions, type NormalizedConfiguration } from "./configuration.js";
import { DaemonProbe } from "./daemon-probe.js";
import { isAgentLaunchError, launchError } from "./errors.js";
import { StageRunner } from "./stage-runner.js";
import type {
  AgentLaunchConnection,
  AgentLauncher,
  ConnectAgentInput,
  CreateAgentLauncherOptions,
  DaemonExit,
  DiscoveryContext,
  SpawnedDaemon,
  StartupLease,
} from "./types.js";

export class DefaultAgentLauncher implements AgentLauncher {
  readonly #configuration: NormalizedConfiguration;
  readonly #options: CreateAgentLauncherOptions;
  readonly #probe: DaemonProbe;
  readonly #stages: StageRunner;

  public constructor(options: CreateAgentLauncherOptions) {
    this.#options = options;
    this.#configuration = validateOptions(options);
    this.#stages = new StageRunner(
      options.ports.clock,
      options.ports.sleeper,
      this.#configuration.pollIntervalMs,
    );
    this.#probe = new DaemonProbe(options, this.#stages);
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
      | { readonly kind: "failed"; readonly error: unknown };
    try {
      const existing = await this.#probe.probe(context, deadline, "READINESS_TIMEOUT", signal);
      if (existing.kind === "ready") {
        outcome = {
          kind: "ready",
          connection: { ...existing.connection, source: "reused" },
        };
      } else {
        this.#stages.throwIfExpired(deadline, "READINESS_TIMEOUT");
        const spawned = await this.#spawn(context.scope, deadline, signal);
        outcome = {
          kind: "ready",
          connection: await this.#waitForReadiness(context, deadline, spawned, signal),
        };
      }
    } catch (error: unknown) {
      outcome = { kind: "failed", error };
    }
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
    spawned: SpawnedDaemon,
    signal?: AbortSignal,
  ): Promise<AgentLaunchConnection> {
    for (;;) {
      this.#stages.throwIfAborted(signal);
      this.#stages.throwIfExpired(deadline, "READINESS_TIMEOUT");
      const ready = await this.#probe.probe(context, deadline, "READINESS_TIMEOUT", signal);
      if (ready.kind === "ready") return { ...ready.connection, source: "spawned" };
      const outcome = await Promise.race([
        spawned.exited.then((exit) => ({ kind: "exit" as const, exit })),
        this.#stages
          .poll(deadline, "READINESS_TIMEOUT", signal)
          .then(() => ({ kind: "poll" as const })),
      ]);
      if (outcome.kind === "exit") this.#throwDaemonExit(outcome.exit);
    }
  }

  async #deriveScope(deadline: number, signal?: AbortSignal): Promise<ScopeIdentity> {
    try {
      return await this.#stages.waitFor(
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
        this.#options.ports.discovery.tryAcquireStartupLease(context),
        deadline,
        "STARTUP_TIMEOUT",
        signal,
      );
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError("unavailable", "CONTROL_PLANE_FAILED", "startup election failed");
    }
  }

  async #releaseLease(lease: StartupLease): Promise<void> {
    try {
      await lease.release();
    } catch {
      throw launchError("unavailable", "CONTROL_PLANE_FAILED", "startup lease release failed");
    }
  }

  async #spawn(
    scope: ScopeIdentity,
    deadline: number,
    signal?: AbortSignal,
  ): Promise<SpawnedDaemon> {
    try {
      return await this.#stages.waitFor(
        this.#options.ports.spawner.spawn({
          canonicalWorkspace: scope.canonicalWorkspace,
          effectiveConfigFingerprint: this.#options.effectiveConfigFingerprint,
          scopeId: scope.scopeId,
        }),
        deadline,
        "READINESS_TIMEOUT",
        signal,
      );
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError("unavailable", "SPAWN_FAILED", "daemon process could not be started");
    }
  }

  #throwDaemonExit(exit: DaemonExit): never {
    const details = exit.exitCode === null ? undefined : { exitCode: exit.exitCode };
    throw launchError("unavailable", "DAEMON_EXITED", "daemon exited before readiness", details);
  }

  async #closeQuietly(connectionClient: AgentLaunchConnection["client"]): Promise<void> {
    try {
      await connectionClient.close();
    } catch {
      // The lease error remains the safe public failure.
    }
  }
}

export function createAgentLauncher(options: CreateAgentLauncherOptions): AgentLauncher {
  return new DefaultAgentLauncher(options);
}
