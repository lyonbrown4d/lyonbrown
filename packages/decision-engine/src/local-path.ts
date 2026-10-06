import { posix, win32 } from "node:path";

export type LocalPathPlatform = "posix" | "win32";

const WINDOWS_DRIVE_ROOT = /^[a-z]:[\\/]$/iu;

export function isFullyQualifiedLocalPath(
  value: string,
  platform: LocalPathPlatform = process.platform === "win32" ? "win32" : "posix",
): boolean {
  if (value.length === 0 || value.includes("\0")) return false;
  if (platform === "win32") {
    return win32.isAbsolute(value) && WINDOWS_DRIVE_ROOT.test(win32.parse(value).root);
  }
  return posix.isAbsolute(value) && !value.startsWith("//");
}
