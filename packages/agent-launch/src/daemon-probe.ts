import { AgentClientError, type AgentClient } from "@lyonbrown/agent-client";
import type { ScopeIdentity } from "@lyonbrown/local-control-plane";
import { isAgentLaunchError, launchError } from "./errors.js";
import type { StageRunner } from "./stage-runner.js";
import { type TimeoutCode } from "./stage-runner.js";
import type {
  AgentLaunchConnection,
  CreateAgentLauncherOptions,
  DiscoveredDaemonCapability,
  DiscoveryContext,
} from "./types.js";

const MAX_EPOCH_TRANSITIONS = 8;

export type ProbeResult =
  | { readonly kind: "unavailable" }
  | { readonly kind: "ready"; readonly connection: Omit<AgentLaunchConnection, "source"> };

export class DaemonProbe {
  public constructor(
    private readonly options: CreateAgentLauncherOptions,
    private readonly stages: StageRunner,
  ) {}

  public async probe(
    context: DiscoveryContext,
    deadline: number,
    timeoutCode: TimeoutCode,
    signal?: AbortSignal,
  ): Promise<ProbeResult> {
    const candidate = await this.#read(context, deadline, timeoutCode, signal);
    if (!candidate) return { kind: "unavailable" };
    const client = this.#createClient(candidate);
    let current = candidate;
    try {
      for (let transition = 0; transition <= MAX_EPOCH_TRANSITIONS; transition += 1) {
        this.stages.throwIfAborted(signal);
        const probe = await this.#authenticateAndSnapshot(
          client,
          current,
          context.scope,
          deadline,
          timeoutCode,
          signal,
        );
        if (probe.kind === "unavailable") {
          await this.#closeQuietly(client);
          return probe;
        }
        const confirmed = await this.#read(context, deadline, timeoutCode, signal);
        if (!confirmed) {
          await this.#closeQuietly(client);
          return { kind: "unavailable" };
        }
        if (confirmed.serverInstanceId === current.serverInstanceId) {
          this.#validateCandidate(confirmed, context.scope);
          return {
            kind: "ready",
            connection: {
              client,
              serverInstanceId: current.serverInstanceId,
              scopeId: current.scopeId,
              snapshot: probe.snapshot,
            },
          };
        }
        if (transition === MAX_EPOCH_TRANSITIONS) {
          throw launchError("unavailable", "EPOCH_UNSTABLE", "daemon epoch did not stabilize");
        }
        this.#replaceBinding(client, confirmed);
        current = confirmed;
      }
      throw launchError("unavailable", "EPOCH_UNSTABLE", "daemon epoch did not stabilize");
    } catch (error: unknown) {
      await this.#closeQuietly(client);
      throw error;
    }
  }

  async #authenticateAndSnapshot(
    client: AgentClient,
    candidate: DiscoveredDaemonCapability,
    expected: ScopeIdentity,
    deadline: number,
    timeoutCode: TimeoutCode,
    signal?: AbortSignal,
  ): Promise<
    | { readonly kind: "unavailable" }
    | { readonly kind: "ready"; readonly snapshot: AgentLaunchConnection["snapshot"] }
  > {
    let handshake;
    try {
      handshake = await this.stages.waitFor(
        () =>
          client.handshake({
            clientName: this.options.client.name,
            clientVersion: this.options.client.version,
          }),
        deadline,
        timeoutCode,
        signal,
      );
    } catch (error: unknown) {
      return this.#mapClientFailure(error);
    }
    if (handshake.serverInstanceId !== candidate.serverInstanceId) {
      throw launchError(
        "protocol",
        "SERVER_INSTANCE_MISMATCH",
        "daemon identity does not match discovery",
      );
    }
    if (handshake.scopeId !== candidate.scopeId) {
      throw launchError("protocol", "SCOPE_MISMATCH", "daemon scope does not match discovery");
    }
    this.#validateCandidate(candidate, expected);
    try {
      return {
        kind: "ready",
        snapshot: await this.stages.waitFor(
          () => client.getSnapshot(),
          deadline,
          timeoutCode,
          signal,
        ),
      };
    } catch (error: unknown) {
      return this.#mapClientFailure(error);
    }
  }

  #mapClientFailure(error: unknown): { readonly kind: "unavailable" } {
    if (isAgentLaunchError(error)) throw error;
    let code: string | undefined;
    try {
      if (error instanceof AgentClientError) {
        const candidate: unknown = error.code;
        if (typeof candidate === "string") code = candidate;
      }
    } catch {
      throw launchError("unavailable", "CLIENT_FAILED", "daemon client operation failed");
    }
    if (code === "AUTHENTICATION_FAILED") {
      throw launchError("authentication", "AUTHENTICATION_FAILED", "daemon authentication failed");
    }
    if (
      code === "DISCONNECTED" ||
      code === "TIMEOUT" ||
      code === "STALE_SERVER_EPOCH" ||
      code === "CANCELLED"
    ) {
      return { kind: "unavailable" };
    }
    throw launchError("unavailable", "CLIENT_FAILED", "daemon client operation failed");
  }

  #validateCandidate(candidate: DiscoveredDaemonCapability, expected: ScopeIdentity): void {
    if (candidate.scopeId !== expected.scopeId) {
      throw launchError("protocol", "SCOPE_MISMATCH", "daemon scope does not match configuration");
    }
  }

  async #read(
    context: DiscoveryContext,
    deadline: number,
    timeoutCode: TimeoutCode,
    signal?: AbortSignal,
  ): Promise<DiscoveredDaemonCapability | null> {
    try {
      return await this.stages.waitFor(
        (operationSignal) => this.options.ports.discovery.read(context, operationSignal),
        deadline,
        timeoutCode,
        signal,
      );
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError("unavailable", "CONTROL_PLANE_FAILED", "secure discovery failed");
    }
  }

  #createClient(candidate: DiscoveredDaemonCapability): AgentClient {
    try {
      return this.options.ports.clients.create(candidate, this.options.deadlines?.client);
    } catch {
      throw launchError("unavailable", "CLIENT_FAILED", "daemon client creation failed");
    }
  }

  #replaceBinding(client: AgentClient, candidate: DiscoveredDaemonCapability): void {
    try {
      this.options.ports.clients.replaceBinding(client, candidate);
    } catch {
      throw launchError("unavailable", "CLIENT_FAILED", "daemon client rebinding failed");
    }
  }

  async #closeQuietly(client: AgentClient): Promise<void> {
    await this.stages.cleanup(() => client.close());
  }
}
