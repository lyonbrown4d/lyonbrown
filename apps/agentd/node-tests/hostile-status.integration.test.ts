import { Metadata, credentials, status, type ServiceError } from "@grpc/grpc-js";
import { SecretToken } from "@lyonbrown/local-control-plane";
import {
  AgentServiceClient,
  GRPC_CHANNEL_OPTIONS,
  StartTurnRequest,
  type CommandResponse,
} from "@lyonbrown/protocol";
import { afterEach, describe, expect, it } from "vitest";
import { startAgentdServer, type RunningAgentdServer } from "../node-src/server.js";
import {
  FakeHost,
  SCOPE_FINGERPRINT,
  SERVER_INSTANCE_ID,
  authorization,
  requestMetadata,
} from "./server-test-support.js";

interface HostileCase {
  readonly name: string;
  readonly create: (accessed: { count: number }) => unknown;
}

const HOST_SECRET_METADATA_KEY = "host-secret";
const HOST_SECRET = "must-not-cross-boundary";

const cases: readonly HostileCase[] = [
  {
    name: "status-shaped object",
    create: () => {
      const metadata = new Metadata();
      metadata.set(HOST_SECRET_METADATA_KEY, HOST_SECRET);
      return { code: status.UNAUTHENTICATED, details: HOST_SECRET, metadata };
    },
  },
  {
    name: "accessor object",
    create: (accessed) =>
      Object.defineProperties(
        {},
        {
          code: { get: () => (accessed.count += 1) },
          details: { get: () => (accessed.count += 1) },
          metadata: { get: () => (accessed.count += 1) },
        },
      ),
  },
  {
    name: "proxy",
    create: (accessed) =>
      new Proxy(
        {},
        {
          get() {
            accessed.count += 1;
            throw new Error(HOST_SECRET);
          },
          getPrototypeOf() {
            accessed.count += 1;
            throw new Error(HOST_SECRET);
          },
          has() {
            accessed.count += 1;
            throw new Error(HOST_SECRET);
          },
        },
      ),
  },
];

describe("RuntimeHost error isolation", () => {
  let running: RunningAgentdServer | undefined;
  let client: AgentServiceClient | undefined;

  afterEach(async () => {
    client?.close();
    await running?.close();
  });

  it.each(cases)(
    "maps a hostile $name to a fixed INTERNAL status without inspection",
    async ({ create }) => {
      const accessed = { count: 0 };
      const host = new FakeHost();
      host.execute.mockRejectedValue(create(accessed));
      const token = SecretToken.generate();
      running = await startAgentdServer({
        host,
        token,
        serverInstanceId: SERVER_INSTANCE_ID,
        scopeFingerprint: SCOPE_FINGERPRINT,
      });
      client = new AgentServiceClient(running.address, credentials.createInsecure(), {
        ...GRPC_CHANNEL_OPTIONS,
      });

      const error = await new Promise<ServiceError>((resolve, reject) => {
        client?.startTurn(
          StartTurnRequest.create({ metadata: requestMetadata("hostile"), text: "hello" }),
          authorization(token),
          { deadline: Date.now() + 500 },
          (callError: ServiceError | null, _response: CommandResponse) => {
            if (callError === null) reject(new Error("hostile error was accepted"));
            else resolve(callError);
          },
        );
      });

      expect(error.code).toBe(status.INTERNAL);
      expect(error.details).toBe("internal daemon error");
      expect(error.metadata.get(HOST_SECRET_METADATA_KEY)).toEqual([]);
      expect(accessed.count).toBe(0);
    },
  );
});
