import { createHash } from "node:crypto";
import { win32 } from "node:path";

import type { NativeModelIdentity } from "@lyonbrown/local-model/composition";

const SAFE_MODEL_ID = /^[a-z0-9](?:[a-z0-9._-]{0,126}[a-z0-9])?$/u;
const COMMIT_REVISION = /^[a-f0-9]{40}$/u;
const SHA256 = /^[a-f0-9]{64}$/u;
const OPERATION_ID = /^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$/u;
const RESERVED_NAMES = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu;
const FORBIDDEN_FILENAME_PUNCTUATION = /[<>:"/\\|?*]/u;

function wellFormed(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!Number.isInteger(next) || next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

function hasForbiddenFilenameUnit(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    if (value.charCodeAt(index) <= 0x1f) return true;
  }
  return FORBIDDEN_FILENAME_PUNCTUATION.test(value);
}

function validFilename(filename: string): boolean {
  return (
    filename.length > 0 &&
    filename.length <= 240 &&
    wellFormed(filename) &&
    win32.basename(filename) === filename &&
    filename !== "." &&
    filename !== ".." &&
    !hasForbiddenFilenameUnit(filename) &&
    !filename.endsWith(".") &&
    !filename.endsWith(" ") &&
    !RESERVED_NAMES.test(filename)
  );
}

export function assertModelIdentity(identity: NativeModelIdentity): void {
  if (
    !SAFE_MODEL_ID.test(identity.modelId) ||
    !COMMIT_REVISION.test(identity.revision) ||
    !SHA256.test(identity.sha256) ||
    !validFilename(identity.filename) ||
    !Number.isSafeInteger(identity.maxBytes) ||
    identity.maxBytes <= 0
  ) {
    throw new Error("native_model_identity_invalid");
  }
}

export function assertOperationId(operationId: string): void {
  if (!OPERATION_ID.test(operationId) || !wellFormed(operationId)) {
    throw new Error("native_model_operation_invalid");
  }
}

export function identityKey(identity: NativeModelIdentity): string {
  assertModelIdentity(identity);
  const hash = createHash("sha256");
  for (const value of [
    identity.modelId,
    identity.revision,
    identity.sha256,
    identity.filename,
    String(identity.maxBytes),
  ]) {
    const bytes = Buffer.from(value, "utf8");
    const length = Buffer.alloc(4);
    length.writeUInt32BE(bytes.length);
    hash.update(length);
    hash.update(bytes);
  }
  return hash.digest("hex");
}
