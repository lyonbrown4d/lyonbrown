import { describe, expect, it } from "vitest";

import { isFullyQualifiedLocalPath } from "@lyonbrown/decision-engine/internal/local-path";

describe("fully-qualified local paths", () => {
  it.each(["/models", "\\models", "C:models", "\\\\server\\share\\models", "\\\\?\\C:\\models"])(
    "rejects ambiguous or non-local Windows path %j",
    (path) => {
      expect(isFullyQualifiedLocalPath(path, "win32")).toBe(false);
    },
  );

  it.each(["F:\\lyonbrown-data\\models", "F:/lyonbrown-data/models"])(
    "accepts fully-qualified drive path %j",
    (path) => {
      expect(isFullyQualifiedLocalPath(path, "win32")).toBe(true);
    },
  );

  it("accepts an absolute POSIX path only on POSIX", () => {
    expect(isFullyQualifiedLocalPath("/var/lib/lyonbrown/models", "posix")).toBe(true);
    expect(isFullyQualifiedLocalPath("relative/models", "posix")).toBe(false);
  });
});
