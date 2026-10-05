import { randomUUID } from "node:crypto";
import { TextDecoder } from "node:util";
import { z } from "zod";
import { validateLoopbackEndpoint } from "./endpoint.ts";
import { ControlPlaneError, controlPlaneError } from "./errors.ts";
import { rejectDuplicateTopLevelKeys } from "./json-keys.ts";
import { SecretToken } from "./token.ts";

export const MAX_DISCOVERY_BYTES = 4 * 1024;
export const DISCOVERY_SCHEMA_VERSION = 1;
export const DISCOVERY_TRANSPORT = "grpc_tcp";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/iu;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";
const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43}$/u;
const SCOPE_PATTERN = /^[a-f0-9]{64}$/u;

const descriptorWireSchema = z
  .object({
    schema_version: z.literal(DISCOVERY_SCHEMA_VERSION),
    transport: z.literal(DISCOVERY_TRANSPORT),
    address: z.string().min(1),
    pid: z.number().int().positive().max(0xffff_ffff),
    server_instance_id: z
      .string()
      .regex(UUID_PATTERN)
      .refine((value) => value !== NIL_UUID),
    token: z.string().regex(TOKEN_PATTERN),
    scope_fingerprint: z.string().regex(SCOPE_PATTERN),
  })
  .strict();

type DescriptorWire = z.infer<typeof descriptorWireSchema>;

export interface DiscoveryDescriptor {
  readonly schemaVersion: 1;
  readonly transport: "grpc_tcp";
  readonly endpoint: string;
  readonly pid: number;
  readonly serverInstanceId: string;
  readonly token: SecretToken;
  readonly scopeId: string;
}

export interface CreateDiscoveryDescriptorInput {
  readonly endpoint: string;
  readonly pid: number;
  readonly scopeId: string;
  readonly serverInstanceId?: string;
  readonly token?: SecretToken;
}

function fromWire(wire: DescriptorWire): DiscoveryDescriptor {
  let token: SecretToken;
  try {
    token = SecretToken.parse(wire.token);
  } catch {
    throw controlPlaneError("INVALID_DESCRIPTOR");
  }
  let endpoint: string;
  try {
    endpoint = validateLoopbackEndpoint(wire.address);
  } catch {
    throw controlPlaneError("INVALID_ENDPOINT");
  }
  return Object.freeze({
    schemaVersion: DISCOVERY_SCHEMA_VERSION,
    transport: DISCOVERY_TRANSPORT,
    endpoint,
    pid: wire.pid,
    serverInstanceId: wire.server_instance_id.toLowerCase(),
    token,
    scopeId: wire.scope_fingerprint,
  });
}

function toWire(descriptor: DiscoveryDescriptor): DescriptorWire {
  return {
    schema_version: descriptor.schemaVersion,
    transport: descriptor.transport,
    address: descriptor.endpoint,
    pid: descriptor.pid,
    server_instance_id: descriptor.serverInstanceId,
    token: descriptor.token.exposeForTransport(),
    scope_fingerprint: descriptor.scopeId,
  };
}

export function createDiscoveryDescriptor(
  input: CreateDiscoveryDescriptorInput,
): DiscoveryDescriptor {
  const candidate = {
    schema_version: DISCOVERY_SCHEMA_VERSION,
    transport: DISCOVERY_TRANSPORT,
    address: input.endpoint,
    pid: input.pid,
    server_instance_id: input.serverInstanceId ?? randomUUID(),
    token: (input.token ?? SecretToken.generate()).exposeForTransport(),
    scope_fingerprint: input.scopeId,
  };
  const parsed = descriptorWireSchema.safeParse(candidate);
  if (!parsed.success) throw controlPlaneError("INVALID_DESCRIPTOR");
  return fromWire(parsed.data);
}

export function serializeDiscoveryDescriptor(descriptor: DiscoveryDescriptor): string {
  const parsed = descriptorWireSchema.safeParse(toWire(descriptor));
  if (!parsed.success) throw controlPlaneError("INVALID_DESCRIPTOR");
  const serialized = JSON.stringify(parsed.data);
  if (Buffer.byteLength(serialized, "utf8") > MAX_DISCOVERY_BYTES) {
    throw controlPlaneError("DESCRIPTOR_TOO_LARGE");
  }
  return serialized;
}

function decodeDescriptor(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? Buffer.from(input, "utf8") : Buffer.from(input);
  if (bytes.length > MAX_DISCOVERY_BYTES) throw controlPlaneError("DESCRIPTOR_TOO_LARGE");
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw controlPlaneError("INVALID_DESCRIPTOR");
  }
}

export function parseDiscoveryDescriptor(input: string | Uint8Array): DiscoveryDescriptor {
  const serialized = decodeDescriptor(input);
  rejectDuplicateTopLevelKeys(serialized);
  let unknownWire: unknown;
  try {
    unknownWire = JSON.parse(serialized) as unknown;
  } catch {
    throw controlPlaneError("INVALID_DESCRIPTOR");
  }
  const parsed = descriptorWireSchema.safeParse(unknownWire);
  if (!parsed.success) throw controlPlaneError("INVALID_DESCRIPTOR");
  try {
    return fromWire(parsed.data);
  } catch (error) {
    if (error instanceof ControlPlaneError) throw error;
    throw controlPlaneError("INVALID_DESCRIPTOR");
  }
}
