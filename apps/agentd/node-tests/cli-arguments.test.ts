import { describe, expect, it } from "vitest";

import {
  parseAgentdArguments,
  type AgentdCliError,
  type AgentdCliOptions,
} from "@lyonbrown/agentd/internal/cli-arguments";

const VALID_ARGUMENTS = [
  "--workspace",
  "C:\\work\\lyonbrown",
  "--effective-config-fingerprint",
  "production-v1",
  "--scope-id",
  "a".repeat(64),
] as const;

function expectCliError(invoke: () => unknown, code: AgentdCliError["code"]): void {
  expect(invoke).toThrow(expect.objectContaining({ code }));
}

describe("agentd CLI arguments", () => {
  it("parses the launcher contract into a frozen typed value", () => {
    const parsed = parseAgentdArguments(VALID_ARGUMENTS);

    expect(parsed).toEqual({
      workspace: "C:\\work\\lyonbrown",
      effectiveConfigFingerprint: "production-v1",
      scopeId: "a".repeat(64),
    } satisfies AgentdCliOptions);
    expect(Object.isFrozen(parsed)).toBe(true);
  });

  it.each(["--workspace", "--effective-config-fingerprint", "--scope-id"] as const)(
    "rejects a missing %s option",
    (missing) => {
      const filtered = VALID_ARGUMENTS.filter(
        (_value, index) =>
          VALID_ARGUMENTS[index] !== missing && VALID_ARGUMENTS[index - 1] !== missing,
      );
      expectCliError(() => parseAgentdArguments(filtered), "CLI_ARGUMENT_MISSING");
    },
  );

  it("rejects a flag without a value", () => {
    expectCliError(
      () =>
        parseAgentdArguments([
          "--workspace",
          "--effective-config-fingerprint",
          "production-v1",
          "--scope-id",
          "a".repeat(64),
        ]),
      "CLI_ARGUMENT_MISSING_VALUE",
    );
  });

  it("rejects duplicate options", () => {
    expectCliError(
      () => parseAgentdArguments([...VALID_ARGUMENTS, "--scope-id", "b".repeat(64)]),
      "CLI_ARGUMENT_DUPLICATE",
    );
  });

  it("rejects unknown options and positional arguments", () => {
    expectCliError(
      () => parseAgentdArguments([...VALID_ARGUMENTS, "--port", "1234"]),
      "CLI_ARGUMENT_UNKNOWN",
    );
    expectCliError(
      () => parseAgentdArguments([...VALID_ARGUMENTS, "positional"]),
      "CLI_ARGUMENT_UNKNOWN",
    );
  });

  it.each(["workspace", "fingerprint", "scope"] as const)("rejects NUL in %s", (field) => {
    const arguments_: string[] = [...VALID_ARGUMENTS];
    const valueIndex = { workspace: 1, fingerprint: 3, scope: 5 }[field];
    arguments_[valueIndex] = `${arguments_[valueIndex] ?? ""}\0suffix`;
    expectCliError(() => parseAgentdArguments(arguments_), "CLI_ARGUMENT_INVALID");
  });

  it("rejects malformed scope ids", () => {
    expectCliError(
      () => parseAgentdArguments([...VALID_ARGUMENTS.slice(0, 5), "not-a-scope"]),
      "CLI_ARGUMENT_INVALID",
    );
  });
});
