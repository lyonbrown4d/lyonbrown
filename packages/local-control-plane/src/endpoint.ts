import { isIP } from "node:net";
import { controlPlaneError } from "./errors.js";

interface EndpointParts {
  readonly host: string;
  readonly port: number;
}

function parsePort(encoded: string): number {
  if (!/^\d{1,5}$/u.test(encoded)) throw controlPlaneError("INVALID_ENDPOINT");
  const port = Number(encoded);
  if (port < 1 || port > 65_535) throw controlPlaneError("INVALID_ENDPOINT");
  return port;
}

function parseEndpoint(endpoint: string): EndpointParts {
  if (endpoint.startsWith("[")) {
    const match = /^\[([^\]]+)\]:(\d{1,5})$/u.exec(endpoint);
    if (match?.[1] === undefined || match[2] === undefined || isIP(match[1]) !== 6) {
      throw controlPlaneError("INVALID_ENDPOINT");
    }
    const normalized = new URL(`http://[${match[1]}]:${match[2]}`);
    if (normalized.hostname !== "[::1]") throw controlPlaneError("INVALID_ENDPOINT");
    return { host: "::1", port: parsePort(match[2]) };
  }

  const separator = endpoint.lastIndexOf(":");
  if (separator <= 0) throw controlPlaneError("INVALID_ENDPOINT");
  const host = endpoint.slice(0, separator);
  const port = parsePort(endpoint.slice(separator + 1));
  if (isIP(host) !== 4 || host.split(".")[0] !== "127") {
    throw controlPlaneError("INVALID_ENDPOINT");
  }
  return { host, port };
}

export function validateLoopbackEndpoint(endpoint: string): string {
  parseEndpoint(endpoint);
  return endpoint;
}
