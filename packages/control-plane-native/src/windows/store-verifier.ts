import { win32 } from "node:path";

import { PathSecurityError, type PathSecurityVerifier } from "@lyonbrown/event-store/path-verifier";

import { api } from "./api.ts";
import { ACCESS, ATTRIBUTES, CREATION, ERRORS, FLAGS, SHARE } from "./constants.ts";
import {
  createFile,
  failureForLastError,
  openDirectory,
  openRegularFile,
  validateObjectType,
  type OwnedHandle,
} from "./handles.ts";
import { fileIdentity } from "./identity.ts";
import { currentUser, verifyPrivateObject, withPrivateSecurity } from "./security.ts";
import { NativeFailure } from "../result.ts";

const DATABASE_NAMES = new Set([
  "lyonbrown.sqlite3",
  "lyonbrown.sqlite3-wal",
  "lyonbrown.sqlite3-shm",
]);

export interface WindowsEventStoreVerifierOptions {
  readonly securityRoot?: string;
}

interface AnchoredObject {
  readonly handle: OwnedHandle;
  readonly identity: string;
}

function safeFailure(): PathSecurityError {
  return new PathSecurityError("Native Windows storage path verification failed");
}

function isWellFormedText(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function pathParts(path: string): { readonly root: string; readonly segments: readonly string[] } {
  if (!win32.isAbsolute(path) || path.includes("\0") || !isWellFormedText(path)) {
    throw safeFailure();
  }
  const parsed = win32.parse(path);
  const segments = path
    .slice(parsed.root.length)
    .split(/[\\/]/u)
    .filter((segment) => segment.length > 0);
  if (segments.some((segment) => segment === "." || segment === "..")) throw safeFailure();
  return { root: parsed.root, segments };
}

function key(path: string): string {
  return win32.normalize(path).toLocaleLowerCase("en-US");
}

function containsPath(root: string, child: string): boolean {
  const relative = win32.relative(root, child);
  return (
    relative === "" ||
    (!win32.isAbsolute(relative) && relative !== ".." && !relative.startsWith(`..${win32.sep}`))
  );
}

function closeHandles(handles: Iterable<OwnedHandle>): void {
  let failed = false;
  for (const handle of handles) {
    try {
      handle.close();
    } catch {
      failed = true;
    }
  }
  if (failed) throw safeFailure();
}

export class WindowsEventStorePathVerifier implements PathSecurityVerifier {
  readonly #configuredRoot: string | undefined;
  readonly #directoryAnchors = new Map<string, AnchoredObject>();
  readonly #fileAnchors = new Map<string, AnchoredObject>();
  #requestedDirectory: string | undefined;
  #closed = false;

  public constructor(options: WindowsEventStoreVerifierOptions = {}) {
    if (options.securityRoot !== undefined) pathParts(options.securityRoot);
    this.#configuredRoot = options.securityRoot && win32.normalize(options.securityRoot);
  }

  public verifyRequestedDataDirectory(requestedDataDirectory: string): void {
    this.#guard(() => {
      this.#assertOpen();
      pathParts(requestedDataDirectory);
      const requested = win32.normalize(requestedDataDirectory);
      const securityRoot = this.#configuredRoot ?? requested;
      if (!containsPath(securityRoot, requested)) throw safeFailure();
      if (this.#requestedDirectory !== undefined) {
        if (key(this.#requestedDirectory) !== key(requested)) throw safeFailure();
        return;
      }
      this.#anchorDirectoryChain(requested, securityRoot);
      this.#requestedDirectory = requested;
    });
  }

  public verifyDataDirectory(
    requestedDataDirectory: string,
    canonicalDataDirectory: string,
  ): string {
    return this.#guard(() => {
      this.verifyRequestedDataDirectory(requestedDataDirectory);
      const requestedAnchor = this.#directoryAnchors.get(key(requestedDataDirectory));
      if (!requestedAnchor) throw safeFailure();
      const canonical = this.#openVerifiedDirectory(canonicalDataDirectory);
      try {
        if (canonical.identity !== requestedAnchor.identity) throw safeFailure();
        return canonical.identity;
      } finally {
        canonical.handle.close();
      }
    });
  }

  public verifyDatabaseFile(
    requestedDataDirectory: string,
    canonicalDataDirectory: string,
    databaseArtifactPath: string,
  ): string | undefined {
    return this.#guard(() => {
      this.verifyDataDirectory(requestedDataDirectory, canonicalDataDirectory);
      this.#validateArtifactPath(canonicalDataDirectory, databaseArtifactPath);
      const artifactKey = key(databaseArtifactPath);
      const anchored = this.#fileAnchors.get(artifactKey);
      if (anchored) return anchored.identity;
      const handle = this.#openArtifact(databaseArtifactPath);
      try {
        validateObjectType(handle, "file");
        verifyPrivateObject(handle.borrow(), currentUser(), false);
        const identity = fileIdentity(handle);
        this.#fileAnchors.set(artifactKey, { handle, identity });
        return identity;
      } catch (error) {
        handle.close();
        throw error;
      }
    });
  }

  public close(): void {
    if (this.#closed) return;
    this.#closed = true;
    const handles = [
      ...[...this.#fileAnchors.values()].map(({ handle }) => handle),
      ...[...this.#directoryAnchors.values()].map(({ handle }) => handle).toReversed(),
    ];
    this.#fileAnchors.clear();
    this.#directoryAnchors.clear();
    closeHandles(handles);
  }

  #anchorDirectoryChain(requested: string, securityRoot: string): void {
    const { root, segments } = pathParts(requested);
    let current = root;
    this.#anchorDirectory(current, false);
    for (const segment of segments) {
      current = win32.join(current, segment);
      this.#anchorDirectory(current, containsPath(securityRoot, current));
    }
  }

  #anchorDirectory(path: string, privateRequired: boolean): void {
    if (this.#directoryAnchors.has(key(path))) return;
    const user = currentUser();
    let handle: OwnedHandle;
    try {
      handle = openDirectory(path, true);
    } catch (error) {
      if (!(error instanceof NativeFailure) || error.failureCode !== "not_found") throw error;
      if (!privateRequired) throw error;
      const created = withPrivateSecurity(user, true, (security) =>
        api.CreateDirectoryW(path, security),
      );
      if (!created && Number(api.GetLastError()) !== ERRORS.alreadyExists) {
        throw failureForLastError();
      }
      handle = openDirectory(path, true);
    }
    try {
      validateObjectType(handle, "directory");
      if (privateRequired) verifyPrivateObject(handle.borrow(), user, true);
      this.#directoryAnchors.set(key(path), { handle, identity: fileIdentity(handle) });
    } catch (error) {
      handle.close();
      throw error;
    }
  }

  #openVerifiedDirectory(path: string): AnchoredObject {
    pathParts(path);
    const handle = openDirectory(path, true);
    try {
      validateObjectType(handle, "directory");
      verifyPrivateObject(handle.borrow(), currentUser(), true);
      return { handle, identity: fileIdentity(handle) };
    } catch (error) {
      handle.close();
      throw error;
    }
  }

  #openArtifact(path: string): OwnedHandle {
    try {
      return openRegularFile(path, ACCESS.genericRead | ACCESS.writeDac, SHARE.read | SHARE.write);
    } catch (error) {
      if (!(error instanceof NativeFailure) || error.failureCode !== "not_found") throw error;
    }
    const user = currentUser();
    return withPrivateSecurity(user, false, (security) =>
      createFile(
        path,
        ACCESS.genericRead | ACCESS.genericWrite | ACCESS.writeDac,
        SHARE.read | SHARE.write,
        CREATION.createNew,
        ATTRIBUTES.normal | FLAGS.openReparsePoint,
        security,
      ),
    );
  }

  #validateArtifactPath(directory: string, artifact: string): void {
    pathParts(directory);
    pathParts(artifact);
    if (
      key(win32.dirname(artifact)) !== key(directory) ||
      !DATABASE_NAMES.has(win32.basename(artifact))
    ) {
      throw safeFailure();
    }
  }

  #assertOpen(): void {
    if (this.#closed) throw safeFailure();
  }

  #guard<T>(operation: () => T): T {
    try {
      return operation();
    } catch (error) {
      if (error instanceof PathSecurityError) throw error;
      throw safeFailure();
    }
  }
}

export function createWindowsEventStorePathVerifier(
  options: WindowsEventStoreVerifierOptions = {},
): PathSecurityVerifier {
  return new WindowsEventStorePathVerifier(options);
}
