import { spawn as nodeSpawn, type SpawnOptions } from "node:child_process";
import type { ProcessSpawner, SpawnAgentdRequest, SpawnAttempt } from "./types.ts";

export interface AgentdProcessConfiguration {
  readonly command: string;
  readonly arguments?: readonly string[];
  readonly workingDirectory?: string;
}

export interface SpawnedAgentdProcess {
  once(event: "error", listener: (error: Error) => void): this;
  once(event: "spawn", listener: () => void): this;
  once(
    event: "exit" | "close",
    listener: (code: number | null, signal: NodeJS.Signals | null) => void,
  ): this;
  kill(): boolean;
  unref(): void;
}

export type SpawnProcess = (
  command: string,
  arguments_: readonly string[],
  options: SpawnOptions,
) => SpawnedAgentdProcess;

interface Deferred<T> {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

class NodeSpawnAttempt implements SpawnAttempt {
  readonly #exited = deferred<{ readonly exitCode: number | null }>();
  readonly #retired = deferred<undefined>();
  readonly exited = this.#exited.promise;
  readonly retired = this.#retired.promise;
  #child: SpawnedAgentdProcess | undefined;
  #state: "pending" | "running" | "retired" = "pending";
  #exitReported = false;
  #spawnConfirmed = false;

  public constructor(
    private readonly configuration: Readonly<
      Required<Pick<AgentdProcessConfiguration, "command">> & {
        readonly arguments: readonly string[];
        readonly workingDirectory?: string;
      }
    >,
    private readonly request: SpawnAgentdRequest,
    private readonly spawnProcess: SpawnProcess,
  ) {
    queueMicrotask(() => {
      this.#start();
    });
  }

  public async abandon(signal: AbortSignal): Promise<void> {
    if (this.#state === "retired") return;
    if (this.#state === "pending") {
      this.#state = "retired";
      this.#reportExit(null);
      this.#retired.resolve(undefined);
      return;
    }
    const child = this.#child;
    if (child === undefined) return;
    child.kill();
    await waitForRetirement(this.retired, signal);
  }

  #start(): void {
    if (this.#state !== "pending") return;
    try {
      const child = this.spawnProcess(
        this.configuration.command,
        [
          ...this.configuration.arguments,
          "--workspace",
          this.request.canonicalWorkspace,
          "--effective-config-fingerprint",
          this.request.effectiveConfigFingerprint,
          "--scope-id",
          this.request.scopeId,
        ],
        {
          ...(this.configuration.workingDirectory === undefined
            ? {}
            : { cwd: this.configuration.workingDirectory }),
          detached: true,
          shell: false,
          stdio: "ignore",
          windowsHide: true,
        },
      );
      this.#child = child;
      this.#state = "running";
      child.once("spawn", () => {
        this.#spawnConfirmed = true;
      });
      child.once("error", () => {
        if (this.#spawnConfirmed) return;
        this.#reportExit(null);
        this.#retire();
      });
      child.once("exit", (code) => {
        this.#reportExit(safeExitCode(code));
      });
      child.once("close", (code) => {
        this.#reportExit(safeExitCode(code));
        this.#retire();
      });
      child.unref();
    } catch {
      this.#reportExit(null);
      this.#retire();
    }
  }

  #reportExit(exitCode: number | null): void {
    if (this.#exitReported) return;
    this.#exitReported = true;
    this.#exited.resolve({ exitCode });
  }

  #retire(): void {
    if (this.#state === "retired") return;
    this.#state = "retired";
    this.#retired.resolve(undefined);
  }
}

function safeExitCode(value: number | null): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : null;
}

function waitForRetirement(retired: Promise<void>, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new Error("spawn abandonment was cancelled"));
  return new Promise<void>((resolve, reject) => {
    const abort = (): void => {
      reject(new Error("spawn abandonment was cancelled"));
    };
    signal.addEventListener("abort", abort, { once: true });
    void retired.then(
      () => {
        signal.removeEventListener("abort", abort);
        resolve();
      },
      () => {
        signal.removeEventListener("abort", abort);
        reject(new Error("spawn retirement failed"));
      },
    );
  });
}

function requiredProcessText(value: string, field: string): string {
  if (value.trim().length === 0 || value.includes("\0")) {
    throw new TypeError(`${field} is invalid`);
  }
  return value;
}

function normalizeConfiguration(configuration: AgentdProcessConfiguration): Readonly<
  Required<Pick<AgentdProcessConfiguration, "command">> & {
    readonly arguments: readonly string[];
    readonly workingDirectory?: string;
  }
> {
  const command = requiredProcessText(configuration.command, "agentd.command");
  const arguments_ = Object.freeze(
    (configuration.arguments ?? []).map((argument, index) =>
      requiredProcessText(argument, `agentd.arguments[${String(index)}]`),
    ),
  );
  const workingDirectory = configuration.workingDirectory;
  if (workingDirectory !== undefined) {
    requiredProcessText(workingDirectory, "agentd.workingDirectory");
  }
  return Object.freeze({
    command,
    arguments: arguments_,
    ...(workingDirectory === undefined ? {} : { workingDirectory }),
  });
}

export class NodeProcessSpawner implements ProcessSpawner {
  readonly #configuration: ReturnType<typeof normalizeConfiguration>;

  public constructor(
    configuration: AgentdProcessConfiguration,
    private readonly spawnProcess: SpawnProcess = nodeSpawn,
  ) {
    this.#configuration = normalizeConfiguration(configuration);
  }

  public begin(request: SpawnAgentdRequest): SpawnAttempt {
    return new NodeSpawnAttempt(this.#configuration, request, this.spawnProcess);
  }
}
