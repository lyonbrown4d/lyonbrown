import { describe, expect, it } from "vitest";

import { parseCliArgs } from "@lyonbrown/agent-cli/internal/cli";

describe("agent command", () => {
  it("enters chat directly with no subcommand", () => {
    expect(parseCliArgs([])).toEqual({ kind: "chat" });
  });

  it("does not retain the legacy chat subcommand", () => {
    expect(() => parseCliArgs(["chat"])).toThrow(/unexpected argument/u);
  });

  it("supports help and version without adding a subcommand", () => {
    expect(parseCliArgs(["--help"]).kind).toBe("help");
    expect(parseCliArgs(["--version"]).kind).toBe("version");
  });
});
