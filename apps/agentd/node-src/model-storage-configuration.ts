import { isAbsolute, normalize } from "node:path";

export const LOCAL_MODEL_ROOT_ENV = "LYONBROWN_LOCAL_MODEL_ROOT";

export function resolveLocalModelStorageDirectory(
  environment: Readonly<NodeJS.ProcessEnv>,
  fallbackDirectory: string,
): string {
  const configured = environment[LOCAL_MODEL_ROOT_ENV];
  if (configured === undefined) return fallbackDirectory;
  if (configured.trim().length === 0 || hasUnsafePathText(configured) || !isAbsolute(configured)) {
    throw new Error("LOCAL_MODEL_ROOT_INVALID");
  }
  return normalize(configured);
}

function hasUnsafePathText(value: string): boolean {
  for (const character of value) {
    const codePoint = character.codePointAt(0);
    if (
      codePoint === undefined ||
      codePoint < 32 ||
      codePoint === 127 ||
      (codePoint >= 0xd800 && codePoint <= 0xdfff)
    ) {
      return true;
    }
  }
  return false;
}
