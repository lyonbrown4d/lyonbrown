import type { ResolvedWorkspaceIdentity } from "@lyonbrown/local-control-plane/native-adapter";
import { api } from "./api.js";
import { FILE_INFO } from "./constants.js";
import { failureForLastError, openDirectory, validateObjectType } from "./handles.js";
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
  const handle = openDirectory(path, false);
  try {
    validateObjectType(handle, "directory");
    const info = Buffer.alloc(24);
    if (!api.GetFileInformationByHandleEx(handle.borrow(), FILE_INFO.id, info, info.length)) {
      throw failureForLastError();
    }
    return {
      canonicalPath: canonicalPath(handle),
      identity: {
        kind: "windows-high-res",
        volumeSerialNumber: info.readBigUInt64LE(0),
        fileId: fileIdFromLittleEndian(info.subarray(8, 24)),
      },
    };
  } finally {
    handle.close();
  }
}
