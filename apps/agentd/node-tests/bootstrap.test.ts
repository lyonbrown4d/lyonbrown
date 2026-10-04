import type { ControlPlaneError } from "@lyonbrown/local-control-plane";
import { describe, expect, it } from "vitest";
import { startProductionAgentd } from "../node-src/bootstrap.js";

describe("production bootstrap", () => {
  it("fails closed before runtime composition when no native provider exists", async () => {
    await expect(
      startProductionAgentd({
        workspace: process.cwd(),
        effectiveConfigFingerprint: "test",
      }),
    ).rejects.toMatchObject({
      code: "NATIVE_SECURITY_PROVIDER_REQUIRED",
    } satisfies Partial<ControlPlaneError>);
  });
});
