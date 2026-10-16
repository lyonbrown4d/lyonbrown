import koffi from "koffi";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

function darwinAclApi() {
  const libc = koffi.load("/usr/lib/libSystem.B.dylib");
  return {
    getAcl: libc.func("void *acl_get_fd_np(int fd, int type)"),
    validAcl: libc.func("int acl_valid(void *acl)"),
    getEntry: libc.func("acl_get_entry", "int", ["void *", "int", "_Out_ void **"]),
    getTag: libc.func("acl_get_tag_type", "int", ["void *", "_Out_ int *"]),
    freeAcl: libc.func("int acl_free(void *acl)"),
  };
}
const api = process.platform === "darwin" ? darwinAclApi() : undefined;

/** Darwin ACL allows can grant access beyond mode bits; Linux ACL masks use group bits. */
export function verifyNoExtendedAcl(fd: number): void {
  verifyAcl(fd, false);
}

/** Ancestors may retain deny-only ACLs, such as the default macOS home ACL. */
export function verifyAncestorAcl(fd: number): void {
  verifyAcl(fd, true);
}

function verifyAcl(fd: number, allowDeny: boolean): void {
  if (!api) return;
  const acl: unknown = api.getAcl(fd, 0x100);
  if (!acl) {
    // An already fstat-verified live fd returns ENOENT when no extended ACL is stored.
    if (koffi.errno() === koffi.os.errno.ENOENT) return;
    throw new NativeFailure("io");
  }
  let failure: NativeFailure | undefined;
  try {
    if (Number(api.validAcl(acl)) !== 0) throw new NativeFailure("insecure_acl");
    // Darwin ACL_FIRST_ENTRY=0, ACL_NEXT_ENTRY=-1; a validated empty/end ACL
    // reports EINVAL. Bound iteration even if native results are malformed.
    for (let index = 0; ; index += 1) {
      if (index >= 1024) throw new NativeFailure("insecure_acl");
      const entry: unknown[] = [null];
      const result = Number(api.getEntry(acl, index === 0 ? 0 : -1, entry));
      if (result === -1 && koffi.errno() === koffi.os.errno.EINVAL) break;
      if (result !== 0) throw new NativeFailure("io");
      if (!allowDeny) throw new NativeFailure("insecure_acl");
      const tag: number[] = [0];
      // ACL_EXTENDED_DENY=2; grants on a mutable path ancestor are rejected.
      if (Number(api.getTag(entry[0], tag)) !== 0 || tag[0] !== 2) {
        throw new NativeFailure("insecure_acl");
      }
    }
  } catch (error) {
    failure = error instanceof NativeFailure ? error : new NativeFailure("io");
  }
  if (Number(api.freeAcl(acl)) !== 0) failure ??= new NativeFailure("io");
  if (failure) throw failure;
}
