import { describe, expect, it } from "vitest";

import { nextTransportRevision } from "../node-src/transport-revision.ts";

describe("daemon transport revisions", () => {
  it("increments valid revisions and rejects negative or exhausted values", () => {
    expect(nextTransportRevision(1n)).toBe(2n);
    expect(() => nextTransportRevision(-1n)).toThrow("runtime snapshot cannot be projected");
    expect(() => nextTransportRevision(0xffff_ffff_ffff_ffffn)).toThrow(
      "runtime transport revision exhausted",
    );
  });
});
