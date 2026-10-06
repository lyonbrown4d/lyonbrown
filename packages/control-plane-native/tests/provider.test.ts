import { describe, expect, it } from "vitest";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native/internal/index";

describe("control-plane native provider", () => {
  it("constructs an opaque provider on the host platform", () => {
    const provider = createControlPlaneNativeProvider();
    expect(provider).toEqual({});
    expect(Object.isFrozen(provider)).toBe(true);
  });
});
