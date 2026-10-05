import type {
  AgentClient,
  AgentCommand,
  RuntimeEventDto,
  RuntimeSnapshotDto,
  SubscribeEventsInput,
} from "@lyonbrown/agent-client";
import type { AgentLaunchConnection, AgentLauncher } from "@lyonbrown/agent-launch";

import { runtimeSnapshot } from "./runtime.ts";

export class FakeAgentClient implements AgentClient {
  public readonly commands: AgentCommand[] = [];
  public readonly events = new PushEvents();
  public readonly subscriptions: SubscribeEventsInput[] = [];
  public closeCount = 0;

  public async health() {
    return { protocolVersion: 1, status: "serving" as const };
  }

  public async handshake() {
    return {
      protocolVersion: 1,
      serverVersion: "0.0.0",
      serverInstanceId: "11111111-1111-4111-8111-111111111111",
      capabilities: [],
      scopeId: "a".repeat(64),
    };
  }

  public async enqueue(command: AgentCommand) {
    this.commands.push(command);
    return { commandId: String(this.commands.length), accepted: true as const };
  }

  public async getSnapshot() {
    return runtimeSnapshot();
  }

  public subscribeEvents(input: SubscribeEventsInput): AsyncIterable<RuntimeEventDto> {
    this.subscriptions.push(input);
    return this.events;
  }

  public replaceBinding(): void {
    // The fake has no transport binding.
  }

  public async close(): Promise<void> {
    this.closeCount += 1;
  }
}

export class FakeLauncher implements AgentLauncher {
  public constructor(private readonly connection: Promise<AgentLaunchConnection>) {}

  public connect(): Promise<AgentLaunchConnection> {
    return this.connection;
  }
}

export function connection(
  client: AgentClient,
  snapshot: RuntimeSnapshotDto = runtimeSnapshot(),
  serverInstanceId = "11111111-1111-4111-8111-111111111111",
): AgentLaunchConnection {
  return {
    client,
    serverInstanceId,
    scopeId: "a".repeat(64),
    snapshot,
    source: "reused",
  };
}

export class PushEvents implements AsyncIterable<RuntimeEventDto> {
  private readonly values: RuntimeEventDto[] = [];
  private readonly waiters: {
    resolve(value: IteratorResult<RuntimeEventDto>): void;
    reject(reason: unknown): void;
  }[] = [];

  public push(value: RuntimeEventDto): void {
    const waiter = this.waiters.shift();
    if (waiter === undefined) this.values.push(value);
    else waiter.resolve({ done: false, value });
  }

  public fail(reason: unknown): void {
    for (const waiter of this.waiters.splice(0)) waiter.reject(reason);
  }

  public complete(): void {
    for (const waiter of this.waiters.splice(0)) waiter.resolve({ done: true, value: undefined });
  }

  public [Symbol.asyncIterator](): AsyncIterator<RuntimeEventDto> {
    return {
      next: () => {
        const value = this.values.shift();
        if (value !== undefined) return Promise.resolve({ done: false, value });
        return new Promise((resolve, reject) => this.waiters.push({ resolve, reject }));
      },
    };
  }
}
