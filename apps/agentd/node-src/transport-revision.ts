import { assertProtocolRevision } from "./runtime-adapter-projection.ts";

const MAX_UINT64 = 0xffff_ffff_ffff_ffffn;

export function nextTransportRevision(current: bigint): bigint {
  const validated = assertProtocolRevision(current);
  if (validated === MAX_UINT64) throw new Error("runtime transport revision exhausted");
  return validated + 1n;
}
