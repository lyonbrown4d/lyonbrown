import type { ResolvedWorkspaceIdentity } from "@lyonbrown/local-control-plane/native-adapter";
import { api } from "./api.js";
import { FILE_INFO } from "./constants.js";
import { failureForLastError, openWorkspaceDirectory, validateObjectType } from "./handles.js";
import type { OwnedHandle } from "./handles.js";

export function canonicalPath(handle: OwnedHandle): string {
  const required = Number(api.GetFinalPathNameByHandleW(handle.borrow(), null, 0, 0));
  if (required <= 0 || required > 32_768) throw failureForLastError();
  const output = Buffer.alloc((required + 1) * 2);
  const written = Number(api.GetFinalPathNameByHandleW(handle.borrow(), output, required + 1, 0));
  if (written <= 0 || written >= required + 1) throw failureForLastError();
  return output.subarray(0, written * 2).toString("utf16le");
}

function fileIdFromLittleEndian(bytes: Buffer): bigint {
  return bytes.readBigUInt64LE(0) | (bytes.readBigUInt64LE(8) << 64n);
}

export function workspaceIdentity(path: string): ResolvedWorkspaceIdentity {
  const handle = openWorkspaceDirectory(path);
  try {
    validateObjectType(handle, "directory");
    const info = Buffer.alloc(24);
    const identity = api.GetFileInformationByHandleEx(
      handle.borrow(),
      FILE_INFO.id,
      info,
      info.length,
    )
      ? {
          kind: "windows-high-res" as const,
          volumeSerialNumber: info.readBigUInt64LE(0),
          fileId: fileIdFromLittleEndian(info.subarray(8, 24)),
        }
      : lowResolutionIdentity(handle);
    return {
      canonicalPath: canonicalPath(handle),
      identity,
    };
  } finally {
    handle.close();
  }
}

function lowResolutionIdentity(handle: OwnedHandle): ResolvedWorkspaceIdentity["identity"] {
  const info = Buffer.alloc(52);
  if (!api.GetFileInformationByHandle(handle.borrow(), info)) throw failureForLastError();
  return {
    kind: "windows-low-res",
    volumeSerialNumber: BigInt(info.readUInt32LE(28)),
    fileIndex: (BigInt(info.readUInt32LE(44)) << 32n) | BigInt(info.readUInt32LE(48)),
  };
}
