import koffi, { type LibraryHandle } from "koffi";

export type Handle = object | bigint;
export type Pointer = object | bigint;

const HANDLE = koffi.pointer("HANDLE", koffi.opaque());
const POINTER = koffi.pointer("LPVOID", koffi.opaque());

export const SECURITY_ATTRIBUTES = koffi.struct("SECURITY_ATTRIBUTES", {
  nLength: "uint32_t",
  lpSecurityDescriptor: POINTER,
  bInheritHandle: "int32_t",
});

export const FILE_ATTRIBUTE_TAG_INFO = koffi.struct("FILE_ATTRIBUTE_TAG_INFO", {
  FileAttributes: "uint32_t",
  ReparseTag: "uint32_t",
});

export const FILE_ID_INFO = koffi.struct("FILE_ID_INFO", {
  VolumeSerialNumber: "uint64_t",
  FileId: koffi.array("uint8_t", 16, "Typed"),
});

export const ACL_SIZE_INFORMATION = koffi.struct("ACL_SIZE_INFORMATION", {
  AceCount: "uint32_t",
  AclBytesInUse: "uint32_t",
  AclBytesFree: "uint32_t",
});

export const OVERLAPPED = koffi.struct("OVERLAPPED", {
  Internal: "uintptr_t",
  InternalHigh: "uintptr_t",
  Offset: "uint32_t",
  OffsetHigh: "uint32_t",
  hEvent: HANDLE,
});

const kernel32 = koffi.load("kernel32.dll");
const advapi32 = koffi.load("advapi32.dll");
const ntdll = koffi.load("ntdll.dll");

type NativeFunction = (...arguments_: unknown[]) => unknown;
function stdcall(library: LibraryHandle, name: string, result: string, args: unknown[]) {
  return library.func("__stdcall", name, result, args as never[]) as NativeFunction;
}

export const api = {
  CreateDirectoryW: stdcall(kernel32, "CreateDirectoryW", "int32_t", [
    "str16",
    koffi.pointer(SECURITY_ATTRIBUTES),
  ]),
  CreateFileW: stdcall(kernel32, "CreateFileW", HANDLE.name, [
    "str16",
    "uint32_t",
    "uint32_t",
    koffi.pointer(SECURITY_ATTRIBUTES),
    "uint32_t",
    "uint32_t",
    HANDLE,
  ]),
  CloseHandle: stdcall(kernel32, "CloseHandle", "int32_t", [HANDLE]),
  GetLastError: stdcall(kernel32, "GetLastError", "uint32_t", []),
  GetCurrentProcess: stdcall(kernel32, "GetCurrentProcess", HANDLE.name, []),
  OpenProcessToken: stdcall(kernel32, "OpenProcessToken", "int32_t", [
    HANDLE,
    "uint32_t",
    koffi.out(koffi.pointer(HANDLE)),
  ]),
  GetTokenInformation: stdcall(advapi32, "GetTokenInformation", "int32_t", [
    HANDLE,
    "uint32_t",
    POINTER,
    "uint32_t",
    koffi.out(koffi.pointer("uint32_t")),
  ]),
  GetLengthSid: stdcall(advapi32, "GetLengthSid", "uint32_t", [POINTER]),
  EqualSid: stdcall(advapi32, "EqualSid", "int32_t", [POINTER, POINTER]),
  ConvertSidToStringSidW: stdcall(advapi32, "ConvertSidToStringSidW", "int32_t", [
    POINTER,
    koffi.out(koffi.pointer(POINTER)),
  ]),
  ConvertStringSecurityDescriptorToSecurityDescriptorW: stdcall(
    advapi32,
    "ConvertStringSecurityDescriptorToSecurityDescriptorW",
    "int32_t",
    ["str16", "uint32_t", koffi.out(koffi.pointer(POINTER)), koffi.pointer("uint32_t")],
  ),
  LocalFree: stdcall(kernel32, "LocalFree", POINTER.name, [POINTER]),
  GetSecurityInfo: stdcall(advapi32, "GetSecurityInfo", "uint32_t", [
    HANDLE,
    "uint32_t",
    "uint32_t",
    koffi.out(koffi.pointer(POINTER)),
    POINTER,
    koffi.out(koffi.pointer(POINTER)),
    POINTER,
    koffi.out(koffi.pointer(POINTER)),
  ]),
  GetSecurityDescriptorControl: stdcall(advapi32, "GetSecurityDescriptorControl", "int32_t", [
    POINTER,
    koffi.out(koffi.pointer("uint16_t")),
    koffi.out(koffi.pointer("uint32_t")),
  ]),
  GetSecurityDescriptorDacl: stdcall(advapi32, "GetSecurityDescriptorDacl", "int32_t", [
    POINTER,
    koffi.out(koffi.pointer("int32_t")),
    koffi.out(koffi.pointer(POINTER)),
    koffi.out(koffi.pointer("int32_t")),
  ]),
  SetSecurityInfo: stdcall(advapi32, "SetSecurityInfo", "uint32_t", [
    HANDLE,
    "uint32_t",
    "uint32_t",
    POINTER,
    POINTER,
    POINTER,
    POINTER,
  ]),
  GetAclInformation: stdcall(advapi32, "GetAclInformation", "int32_t", [
    POINTER,
    koffi.out(koffi.pointer(ACL_SIZE_INFORMATION)),
    "uint32_t",
    "uint32_t",
  ]),
  GetAce: stdcall(advapi32, "GetAce", "int32_t", [
    POINTER,
    "uint32_t",
    koffi.out(koffi.pointer(POINTER)),
  ]),
  GetFileInformationByHandleEx: stdcall(kernel32, "GetFileInformationByHandleEx", "int32_t", [
    HANDLE,
    "uint32_t",
    POINTER,
    "uint32_t",
  ]),
  GetFileInformationByHandle: stdcall(kernel32, "GetFileInformationByHandle", "int32_t", [
    HANDLE,
    POINTER,
  ]),
  GetFinalPathNameByHandleW: stdcall(kernel32, "GetFinalPathNameByHandleW", "uint32_t", [
    HANDLE,
    POINTER,
    "uint32_t",
    "uint32_t",
  ]),
  GetFileSizeEx: stdcall(kernel32, "GetFileSizeEx", "int32_t", [
    HANDLE,
    koffi.out(koffi.pointer("int64_t")),
  ]),
  ReadFile: stdcall(kernel32, "ReadFile", "int32_t", [
    HANDLE,
    POINTER,
    "uint32_t",
    koffi.out(koffi.pointer("uint32_t")),
    POINTER,
  ]),
  WriteFile: stdcall(kernel32, "WriteFile", "int32_t", [
    HANDLE,
    POINTER,
    "uint32_t",
    koffi.out(koffi.pointer("uint32_t")),
    POINTER,
  ]),
  FlushFileBuffers: stdcall(kernel32, "FlushFileBuffers", "int32_t", [HANDLE]),
  SetFileInformationByHandle: stdcall(kernel32, "SetFileInformationByHandle", "int32_t", [
    HANDLE,
    "uint32_t",
    POINTER,
    "uint32_t",
  ]),
  NtSetInformationFile: stdcall(ntdll, "NtSetInformationFile", "int32_t", [
    HANDLE,
    POINTER,
    POINTER,
    "uint32_t",
    "uint32_t",
  ]),
  LockFileEx: stdcall(kernel32, "LockFileEx", "int32_t", [
    HANDLE,
    "uint32_t",
    "uint32_t",
    "uint32_t",
    "uint32_t",
    koffi.pointer(OVERLAPPED),
  ]),
  UnlockFileEx: stdcall(kernel32, "UnlockFileEx", "int32_t", [
    HANDLE,
    "uint32_t",
    "uint32_t",
    "uint32_t",
    koffi.pointer(OVERLAPPED),
  ]),
} as const;

export function pointerAddress(value: Handle | Pointer): bigint {
  return koffi.address(value);
}

export function pointerBytes(value: Pointer, length: number): Uint8Array {
  return new Uint8Array(koffi.view(value, length));
}

export function decodeWide(value: Pointer): string {
  return koffi.decode.string16(value);
}

export const pointerSize = koffi.sizeof("void *");
