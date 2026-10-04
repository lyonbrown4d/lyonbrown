import { domainError } from "./errors.js";

export const MAX_AGENT_RUNTIME_ASSISTANT_BYTES = 1024 * 1024;

export function checkedAssistantBytes(current: number, delta: string): number {
  const bytes = Buffer.byteLength(delta, "utf8");
  if (bytes > MAX_AGENT_RUNTIME_ASSISTANT_BYTES - current) {
    throw domainError("resource_exhausted", "Runtime assistant text byte budget exceeded", {
      resource: "runtime_assistant_text_bytes",
      limitBytes: MAX_AGENT_RUNTIME_ASSISTANT_BYTES,
    });
  }
  return current + bytes;
}

export function checkedUsageAdd(current: number, delta: number): number {
  if (delta > Number.MAX_SAFE_INTEGER - current) {
    throw domainError("resource_exhausted", "Runtime usage exceeds safe integer range", {
      resource: "runtime_usage",
      limit: BigInt(Number.MAX_SAFE_INTEGER),
    });
  }
  return current + delta;
}
