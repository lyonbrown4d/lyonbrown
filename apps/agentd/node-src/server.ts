import { Server, ServerCredentials, type ServerOptions } from "@grpc/grpc-js";
import type { SecretToken } from "@lyonbrown/local-control-plane";
import {
  AgentServiceService,
  GRPC_CHANNEL_OPTIONS,
  MAX_GRPC_MESSAGE_BYTES,
} from "@lyonbrown/protocol";
import { createAuthorizationInterceptor } from "./auth.js";
import type { RuntimeHost } from "./runtime-host.js";
import { createAgentService } from "./service.js";

const LOOPBACK_HOST = "127.0.0.1";

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

function shutdown(server: Server): Promise<void> {
  return new Promise((resolve, reject) => {
    server.tryShutdown((error) => {
      if (error === undefined) resolve();
      else reject(error);
    });
  });
}

export async function startAgentdServer(
  options: StartAgentdServerOptions,
): Promise<RunningAgentdServer> {
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
      await shutdown(server);
    },
  });
}

export { MAX_GRPC_MESSAGE_BYTES };
