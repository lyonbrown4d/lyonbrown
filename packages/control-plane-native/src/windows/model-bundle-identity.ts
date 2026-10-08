import { createHash } from "node:crypto";

import type {
  NativeModelBundleFileIdentity,
  NativeModelBundleIdentity,
} from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import { assertOperationId } from "@lyonbrown/control-plane-native/internal/windows/model-identity";

const IDENTIFIER = /^[a-z0-9][a-z0-9._-]{0,127}$/u;
const REVISION = /^[a-f0-9]{40}$/u;
const SHA256 = /^[a-f0-9]{64}$/u;
const MAX_FILES = 32;
const MAX_PATH_BYTES = 256;

function invalid(): never {
  throw new Error("native_model_bundle_identity_invalid");
}

function isArray(value: unknown): value is readonly unknown[] {
  return Array.isArray(value);
}

function wellFormedWithoutControlCharacters(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit <= 0x1f) return false;
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!Number.isInteger(next) || next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

function validSegment(segment: string): boolean {
  return (
    segment.length > 0 &&
    segment !== "." &&
    segment !== ".." &&
    wellFormedWithoutControlCharacters(segment) &&
    !/[<>:"|?*\\]/u.test(segment) &&
    !segment.endsWith(".") &&
    !segment.endsWith(" ") &&
    !/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu.test(segment)
  );
}

function assertFile(file: NativeModelBundleFileIdentity): void {
  if (
    typeof file.path !== "string" ||
    Buffer.byteLength(file.path, "utf8") > MAX_PATH_BYTES ||
    file.path.startsWith("/") ||
    file.path.endsWith("/") ||
    file.path.split("/").some((segment) => !validSegment(segment)) ||
    !SHA256.test(file.sha256) ||
    !Number.isSafeInteger(file.maxBytes) ||
    file.maxBytes <= 0
  ) {
    invalid();
  }
}

export function assertModelBundleIdentity(identity: NativeModelBundleIdentity): void {
  if (
    !IDENTIFIER.test(identity.modelId) ||
    !REVISION.test(identity.revision) ||
    !isArray(identity.files) ||
    identity.files.length === 0 ||
    identity.files.length > MAX_FILES
  ) {
    invalid();
  }
  const paths = new Set<string>();
  for (const file of identity.files) {
    assertFile(file);
    const key = file.path.toLocaleLowerCase("en-US");
    if (paths.has(key)) invalid();
    paths.add(key);
  }
}

export function assertBundleOperationId(operationId: string): void {
  assertOperationId(operationId);
}

export function modelBundleKey(identity: NativeModelBundleIdentity): string {
  assertModelBundleIdentity(identity);
  const canonical = JSON.stringify({
    modelId: identity.modelId,
    revision: identity.revision,
    files: identity.files.map(({ path, sha256, maxBytes }) => ({ path, sha256, maxBytes })),
  });
  return createHash("sha256").update(canonical).digest("hex");
}

export function bundleIndexName(identity: NativeModelBundleIdentity): string {
  return `.bundle-index-${modelBundleKey(identity)}.json`;
}

export function stagingBundleName(
  identity: NativeModelBundleIdentity,
  randomSuffix: string,
): string {
  return `.staging-${modelBundleKey(identity)}-${randomSuffix}`;
}

export function assertActiveBundleDirectoryName(
  identity: NativeModelBundleIdentity,
  value: unknown,
): string {
  const prefix = `.staging-${modelBundleKey(identity)}-`;
  if (
    typeof value !== "string" ||
    !value.startsWith(prefix) ||
    value.length !== prefix.length + 32 ||
    !/^[a-f0-9]+$/u.test(value.slice(prefix.length))
  ) {
    invalid();
  }
  return value;
}
