import { createHash } from "node:crypto";
import { isAbsolute } from "node:path";

import type { NativeModelIdentity } from "@lyonbrown/local-model/composition";
import type { NativeModelBundleIdentity } from "@lyonbrown/control-plane-native/internal/model-bundle-storage";

const MODEL_ID = /^[a-z0-9][a-z0-9._-]{0,127}$/u;
const REVISION = /^[a-f0-9]{40}$/u;
const SHA256 = /^[a-f0-9]{64}$/u;

function segment(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= 240 &&
    wellFormed(value) &&
    value !== "." &&
    value !== ".." &&
    !/[<>:"/\\|?*]/u.test(value) &&
    !/[. ]$/u.test(value) &&
    !/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu.test(value)
  );
}

function wellFormed(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit <= 0x1f) return false;
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!Number.isInteger(next) || next < 0xdc00 || next > 0xdfff) return false;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

export function assertModelIdentity(value: NativeModelIdentity): void {
  if (
    !/^[a-z0-9](?:[a-z0-9._-]{0,126}[a-z0-9])?$/u.test(value.modelId) ||
    !REVISION.test(value.revision) ||
    !SHA256.test(value.sha256) ||
    !segment(value.filename) ||
    !Number.isSafeInteger(value.maxBytes) ||
    value.maxBytes <= 0
  ) {
    throw new Error("native_model_identity_invalid");
  }
}

export function assertOperationId(value: string): void {
  if (!/^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$/u.test(value)) {
    throw new Error("native_model_operation_invalid");
  }
}

export function validateModelsPath(value: string): string {
  if (
    !isAbsolute(value) ||
    value === "/" ||
    !wellFormed(value) ||
    value.includes("\0") ||
    value
      .split("/")
      .slice(1)
      .some((part) => !segment(part))
  ) {
    throw new Error("native_model_directory_invalid");
  }
  return value;
}

export function modelKey(identity: NativeModelIdentity): string {
  assertModelIdentity(identity);
  const hash = createHash("sha256");
  for (const value of [
    identity.modelId,
    identity.revision,
    identity.sha256,
    identity.filename,
    String(identity.maxBytes),
  ]) {
    const bytes = Buffer.from(value);
    const length = Buffer.alloc(4);
    length.writeUInt32BE(bytes.length);
    hash.update(length).update(bytes);
  }
  return hash.digest("hex");
}

export function assertBundleIdentity(identity: NativeModelBundleIdentity): void {
  if (
    !MODEL_ID.test(identity.modelId) ||
    !REVISION.test(identity.revision) ||
    !isArray(identity.files) ||
    identity.files.length < 1 ||
    identity.files.length > 32
  ) {
    throw new Error("native_model_bundle_identity_invalid");
  }
  const paths = new Set<string>();
  for (const file of identity.files) {
    const key = file.path.toLocaleLowerCase("en-US");
    if (
      Buffer.byteLength(file.path) > 256 ||
      file.path.split("/").some((part) => !segment(part)) ||
      !SHA256.test(file.sha256) ||
      !Number.isSafeInteger(file.maxBytes) ||
      file.maxBytes <= 0 ||
      paths.has(key)
    )
      throw new Error("native_model_bundle_identity_invalid");
    paths.add(key);
  }
  for (const path of paths) {
    if ([...paths].some((other) => other.startsWith(`${path}/`))) {
      throw new Error("native_model_bundle_identity_invalid");
    }
  }
}

function isArray(value: unknown): value is readonly unknown[] {
  return Array.isArray(value);
}

export function bundleKey(identity: NativeModelBundleIdentity): string {
  assertBundleIdentity(identity);
  return createHash("sha256")
    .update(
      JSON.stringify({
        modelId: identity.modelId,
        revision: identity.revision,
        files: identity.files.map(({ path, sha256, maxBytes }) => ({ path, sha256, maxBytes })),
      }),
    )
    .digest("hex");
}
