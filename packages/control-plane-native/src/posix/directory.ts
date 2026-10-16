import { fstatSync } from "node:fs";
import { posix } from "node:path";
import { verifyAncestorAcl } from "@lyonbrown/control-plane-native/internal/posix/acl";
import {
  closeFd,
  FLAGS,
  mkdirAt,
  openAt,
  openRoot,
  sameIdentity,
  verifyPrivate,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

export class PosixDirectory {
  #fd: number | undefined;
  readonly #parent: PosixDirectory | undefined;
  readonly #name: string;
  readonly #private: boolean;
  readonly #ownsParent: boolean;

  public constructor(
    fd: number,
    public readonly path: string,
    parent?: PosixDirectory,
    name = "",
    privateDirectory = false,
    ownsParent = false,
  ) {
    this.#fd = fd;
    this.#parent = parent;
    this.#name = name;
    this.#private = privateDirectory;
    this.#ownsParent = ownsParent;
  }

  public get fd(): number {
    if (this.#fd === undefined) throw new NativeFailure("io");
    return this.#fd;
  }

  public assertIdentity(): void {
    const fd = this.fd;
    if (this.#private) {
      verifyPrivate(fd, true);
      this.#assertTrustedAncestors();
    }
    if (!this.#parent) return;
    this.#parent.assertIdentity();
    const current = openAt(this.#parent.fd, this.#name, FLAGS.readOnly | FLAGS.directory);
    try {
      if (!sameIdentity(fd, current)) throw new NativeFailure("io");
    } finally {
      closeFd(current);
    }
  }

  #assertTrustedAncestors(): void {
    // Canonical-path callbacks on Darwin also require an ancestor chain that
    // other OS users cannot rename. Root/current-user sticky directories (e.g.
    // /tmp) protect owned entries; arbitrary writable parents do not.
    const uid = process.getuid?.();
    if (uid === undefined) throw new NativeFailure("io");
    const stat = fstatSync(this.fd, { bigint: true });
    // An unrelated owner can rename children even when group/other write is off.
    if (stat.uid !== 0n && stat.uid !== BigInt(uid)) throw new NativeFailure("wrong_owner");
    const stickyOwner =
      (stat.mode & 0o1000n) !== 0n && (stat.uid === 0n || stat.uid === BigInt(uid));
    if ((stat.mode & 0o022n) !== 0n && !stickyOwner) {
      throw new NativeFailure("insecure_mode");
    }
    verifyAncestorAcl(this.fd);
    if (this.#parent) this.#parent.#assertTrustedAncestors();
  }

  public close(): void {
    const fd = this.#fd;
    if (fd === undefined) return;
    this.#fd = undefined;
    try {
      closeFd(fd);
    } finally {
      if (this.#ownsParent) this.#parent?.close();
    }
  }
}

function childDirectory(
  parent: PosixDirectory,
  name: string,
  create: boolean,
  privateLeaf: boolean,
  ownsParent: boolean,
): PosixDirectory {
  let fd: number;
  let created = false;
  try {
    fd = openAt(parent.fd, name, FLAGS.readOnly | FLAGS.directory);
  } catch (error) {
    if (!create || !(error instanceof NativeFailure) || error.failureCode !== "not_found") {
      throw error;
    }
    mkdirAt(parent.fd, name);
    fd = openAt(parent.fd, name, FLAGS.readOnly | FLAGS.directory);
    created = true;
  }
  try {
    if (!fstatSync(fd).isDirectory()) throw new NativeFailure("not_directory");
    if (privateLeaf || created) verifyPrivate(fd, true);
    return new PosixDirectory(
      fd,
      posix.join(parent.path, name),
      parent,
      name,
      privateLeaf || created,
      ownsParent,
    );
  } catch (error) {
    closeFd(fd);
    throw error;
  }
}

export function openDirectoryAt(
  parent: PosixDirectory,
  name: string,
  create = false,
  privateLeaf = true,
): PosixDirectory {
  parent.assertIdentity();
  const child = childDirectory(parent, name, create, privateLeaf, false);
  try {
    child.assertIdentity();
    return child;
  } catch (error) {
    child.close();
    throw error;
  }
}

export function openDirectory(path: string, create = false, privateLeaf = false): PosixDirectory {
  if (!posix.isAbsolute(path) || path.includes("\0")) throw new NativeFailure("io");
  const segments = path.split("/").filter(Boolean);
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw new NativeFailure("io");
  }
  let current = new PosixDirectory(
    openRoot(),
    "/",
    undefined,
    "",
    segments.length === 0 && privateLeaf,
  );
  try {
    // Walking is deliberately sequential: each next open is relative to the held parent fd.
    for (const [index, segment] of segments.entries()) {
      current = childDirectory(
        current,
        segment,
        create,
        privateLeaf && index === segments.length - 1,
        true,
      );
    }
    current.assertIdentity();
    return current;
  } catch (error) {
    current.close();
    throw error;
  }
}

export function openRegularAt(
  directory: PosixDirectory,
  name: string,
  create = false,
  exclusive = false,
): number {
  directory.assertIdentity();
  const fd = openAt(
    directory.fd,
    name,
    FLAGS.nonBlock |
      (create ? FLAGS.readWrite | FLAGS.create : FLAGS.readOnly) |
      (exclusive ? FLAGS.exclusive : 0),
  );
  try {
    verifyPrivate(fd, false);
    directory.assertIdentity();
    return fd;
  } catch (error) {
    closeFd(fd);
    throw error;
  }
}
