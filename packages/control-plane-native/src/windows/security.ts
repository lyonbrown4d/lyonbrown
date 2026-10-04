import koffi from "koffi";
import {
  api,
  decodeWide,
  pointerAddress,
  pointerBytes,
  SECURITY_ATTRIBUTES,
  type Handle,
  type Pointer,
} from "./api.js";
import { SECURITY } from "./constants.js";
import { failureForLastError, OwnedHandle } from "./handles.js";
import { NativeFailure } from "../result.js";

interface CurrentUser {
  readonly sid: Uint8Array;
  readonly sidString: string;
}

function pointerFromBuffer(buffer: Buffer): Pointer {
  const address =
    process.arch === "ia32" ? BigInt(buffer.readUInt32LE(0)) : buffer.readBigUInt64LE(0);
  return address as Pointer;
}

export function currentUser(): CurrentUser {
  const tokenOutput: unknown[] = [null];
  if (!api.OpenProcessToken(api.GetCurrentProcess(), SECURITY.tokenQuery, tokenOutput)) {
    throw failureForLastError();
  }
  const token = new OwnedHandle(tokenOutput[0]);
  try {
    const required: unknown[] = [0];
    api.GetTokenInformation(token.borrow(), 1, null, 0, required);
    const length = Number(required[0]);
    if (length <= 0 || length > 65_536) throw new NativeFailure("io");
    const information = Buffer.alloc(length);
    if (!api.GetTokenInformation(token.borrow(), 1, information, length, required)) {
      throw failureForLastError();
    }
    const sidPointer = pointerFromBuffer(information);
    const sidLength = Number(api.GetLengthSid(sidPointer));
    if (sidLength <= 0 || sidLength > length) throw new NativeFailure("io");
    const sid = Uint8Array.from(pointerBytes(sidPointer, sidLength));
    const stringOutput: unknown[] = [null];
    if (!api.ConvertSidToStringSidW(sidPointer, stringOutput)) throw failureForLastError();
    const allocated = stringOutput[0] as Pointer;
    try {
      return { sid, sidString: decodeWide(allocated) };
    } finally {
      api.LocalFree(allocated);
    }
  } finally {
    token.close();
  }
}

export function withPrivateSecurity<T>(
  user: CurrentUser,
  inheritable: boolean,
  operation: (attributes: object) => T,
): T {
  const aceFlags = inheritable ? "OICI" : "";
  const descriptorOutput: unknown[] = [null];
  const sddl = `D:P(A;${aceFlags};FA;;;${user.sidString})`;
  if (
    !api.ConvertStringSecurityDescriptorToSecurityDescriptorW(
      sddl,
      SECURITY.sddlRevision1,
      descriptorOutput,
      null,
    )
  ) {
    throw failureForLastError();
  }
  const descriptor = descriptorOutput[0] as Pointer;
  try {
    return operation({
      nLength: koffi.sizeof(SECURITY_ATTRIBUTES),
      lpSecurityDescriptor: descriptor,
      bInheritHandle: 0,
    });
  } finally {
    api.LocalFree(descriptor);
  }
}

function sameSid(pointer: Pointer, expected: Uint8Array): boolean {
  return Boolean(api.EqualSid(pointer, Buffer.from(expected)));
}

export function validateAccessAce(
  header: Uint8Array,
  sidMatches: boolean,
  inheritable: boolean,
): void {
  const requiredFlags = inheritable ? 0x03 : 0;
  if (
    header.length < 8 ||
    header[0] !== SECURITY.accessAllowedAce ||
    (header[1] ?? 0) !== requiredFlags ||
    Buffer.from(header).readUInt32LE(4) !== SECURITY.fileAllAccess ||
    !sidMatches
  ) {
    throw new NativeFailure("insecure_acl");
  }
}

function verifyAcl(dacl: Pointer, user: CurrentUser, inheritable: boolean): void {
  const size = { AceCount: 0, AclBytesInUse: 0, AclBytesFree: 0 };
  if (!api.GetAclInformation(dacl, size, koffi.sizeof("ACL_SIZE_INFORMATION"), 2)) {
    throw failureForLastError();
  }
  if (size.AceCount !== 1) throw new NativeFailure("insecure_acl");
  const output: unknown[] = [null];
  if (!api.GetAce(dacl, 0, output)) throw failureForLastError();
  const ace = output[0] as Pointer;
  const header = pointerBytes(ace, 8);
  validateAccessAce(header, sameSid((pointerAddress(ace) + 8n) as Pointer, user.sid), inheritable);
}

export function verifyPrivateObject(handle: Handle, user: CurrentUser, inheritable: boolean): void {
  const owner: unknown[] = [null];
  const dacl: unknown[] = [null];
  const descriptor: unknown[] = [null];
  const status = Number(
    api.GetSecurityInfo(
      handle,
      SECURITY.seFileObject,
      SECURITY.ownerInformation | SECURITY.daclInformation,
      owner,
      null,
      dacl,
      null,
      descriptor,
    ),
  );
  if (status !== 0) throw new NativeFailure(status === 5 ? "access_denied" : "io");
  const allocated = descriptor[0] as Pointer;
  try {
    if (!sameSid(owner[0] as Pointer, user.sid)) throw new NativeFailure("wrong_owner");
    if (!dacl[0]) throw new NativeFailure("insecure_acl");
    const control: unknown[] = [0];
    const revision: unknown[] = [0];
    if (!api.GetSecurityDescriptorControl(allocated, control, revision)) {
      throw failureForLastError();
    }
    if ((Number(control[0]) & SECURITY.protectedDacl) === 0) {
      throw new NativeFailure("insecure_acl");
    }
    verifyAcl(dacl[0] as Pointer, user, inheritable);
  } finally {
    api.LocalFree(allocated);
  }
}
