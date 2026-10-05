import { Server, ServerCredentials, type ServerOptions } from "@grpc/grpc-js";
import type { SecretToken } from "@lyonbrown/local-control-plane";
import {
  AgentServiceService,
  GRPC_CHANNEL_OPTIONS,
  MAX_GRPC_MESSAGE_BYTES,
} from "@lyonbrown/protocol";
import { createAuthorizationInterceptor } from "./auth.ts";
import type { RuntimeHost } from "./runtime-host.ts";
import { createAgentService } from "./service.ts";

const LOOPBACK_HOST = "127.0.0.1";
const DEFAULT_SHUTDOWN_TIMEOUT_MS = 5_000;

export const AGENTD_SERVER_OPTIONS = Object.freeze({
  ...GRPC_CHANNEL_OPTIONS,
}) satisfies ServerOptions;

export interface StartAgentdServerOptions {
  readonly host: RuntimeHost;
  readonly token: SecretToken;
  readonly serverInstanceId: string;
  readonly scopeFingerprint: string;
  readonly replayCapacity?: number;
  readonly port?: number;
  readonly shutdownTimeoutMs?: number;
}

export interface RunningAgentdServer {
  readonly address: string;
  readonly port: number;
  close(): Promise<void>;
}

function bind(server: Server, port: number): Promise<number> {
  return new Promise((resolve, reject) => {
    server.bindAsync(
      `${LOOPBACK_HOST}:${String(port)}`,
      ServerCredentials.createInsecure(),
      (error, boundPort) => {
        if (error === null) resolve(boundPort);
        else reject(error);
      },
    );
  });
}

export interface GrpcShutdownHandle {
  tryShutdown(callback: (error?: Error) => void): void;
  forceShutdown(): void;
}

export async function shutdownServer(server: GrpcShutdownHandle, timeoutMs: number): Promise<void> {
  normalizeShutdownTimeoutMs(timeoutMs);
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    const finish = (error?: unknown): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error === undefined) resolve();
      else reject(error instanceof Error ? error : new Error("gRPC server shutdown failed"));
    };
    const timer = setTimeout(() => {
      try {
        server.forceShutdown();
        finish();
      } catch (error: unknown) {
        finish(error);
      }
    }, timeoutMs);
    timer.unref();
    try {
      server.tryShutdown(finish);
    } catch (error: unknown) {
      finish(error);
    }
  });
}

export async function startAgentdServer(
  options: StartAgentdServerOptions,
): Promise<RunningAgentdServer> {
  const shutdownTimeoutMs = normalizeShutdownTimeoutMs(
    options.shutdownTimeoutMs ?? DEFAULT_SHUTDOWN_TIMEOUT_MS,
  );
  const shutdownController = new AbortController();
  const server = new Server({
    ...AGENTD_SERVER_OPTIONS,
    interceptors: [createAuthorizationInterceptor(options.token)],
  });
  server.addService(
    AgentServiceService,
    createAgentService({
      host: options.host,
      serverInstanceId: options.serverInstanceId,
      scopeFingerprint: options.scopeFingerprint,
      ...(options.replayCapacity === undefined ? {} : { replayCapacity: options.replayCapacity }),
      shutdownSignal: shutdownController.signal,
    }),
  );
  const port = await bind(server, options.port ?? 0);
  let closed = false;
  return Object.freeze({
    address: `${LOOPBACK_HOST}:${String(port)}`,
    port,
    async close() {
      if (closed) return;
      closed = true;
      shutdownController.abort();
      await shutdownServer(server, shutdownTimeoutMs);
    },
  });
}

function normalizeShutdownTimeoutMs(value: number): number {
  if (!Number.isSafeInteger(value) || value < 1 || value > 60_000) {
    throw new RangeError("shutdownTimeoutMs must be a finite integer from 1 to 60000");
  }
  return value;
}

export { MAX_GRPC_MESSAGE_BYTES };
