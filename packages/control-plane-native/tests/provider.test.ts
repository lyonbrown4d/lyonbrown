import { describe, expect, it } from "vitest";
import { createControlPlaneNativeProvider } from "../src/index.js";

describe("control-plane native provider", () => {
  it("constructs an opaque provider on the host platform", () => {
    const provider = createControlPlaneNativeProvider();
    expect(provider).toEqual({});
    expect(Object.isFrozen(provider)).toBe(true);
  });
});
