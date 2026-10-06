import { describe, expect, it } from "vitest";

import {
  adaptPiTool,
  type PiToolBoundary,
} from "@lyonbrown/agentd/internal/workspace-tool-boundaries";

function piTool(result: unknown, parameters: unknown = { type: "object" }): PiToolBoundary {
  return {
    name: "test",
    label: "test",
    description: "test files",
    parameters,
    execute: async () => result,
  };
}

function execute(result: unknown) {
  return adaptPiTool(piTool(result), "low", ["workspace.read"]).execute({
    callId: "call-1",
    arguments: {},
    signal: new AbortController().signal,
  });
}

describe("workspace tool adapter boundaries", () => {
  it("copies bounded JSON details and joins text blocks", async () => {
    const tool = adaptPiTool(
      piTool({
        content: [
          { type: "text", text: "first" },
          { type: "text", text: "second" },
        ],
        details: {
          count: 2,
          active: true,
          empty: null,
          values: ["safe", 3],
        },
      }),
      "medium",
      ["workspace.write"],
    );

    expect([tool.name, tool.label, tool.risk, tool.permissions]).toEqual([
      "test",
      "test",
      "medium",
      ["workspace.write"],
    ]);
    await expect(
      tool.execute({
        callId: "details",
        arguments: {},
        signal: new AbortController().signal,
      }),
    ).resolves.toEqual({
      content: "first\nsecond",
      details: { count: 2, active: true, empty: null, values: ["safe", 3] },
    });
  });

  it.each([null, "text", {}, { type: "image", text: "unsupported" }, { type: "text", text: 42 }])(
    "rejects unsupported Pi content %#",
    async (block) => {
      await expect(execute({ content: [block] })).rejects.toThrow(
        "workspace tool returned unsupported content",
      );
    },
  );

  it("maps Pi tool failures to a stable error", async () => {
    await expect(execute({ content: [], isError: true })).rejects.toThrow("workspace tool failed");
  });

  it.each([undefined, Number.NaN, new Date(), Object.setPrototypeOf({ marker: true }, null)])(
    "handles the optional details boundary %#",
    async (details: unknown) => {
      if (details === undefined) {
        await expect(execute({ content: [], details })).resolves.toEqual({ content: "" });
        return;
      }
      await expect(execute({ content: [], details })).rejects.toThrow(
        "workspace tool details are invalid",
      );
    },
  );

  it("rejects excessively deep tool details", async () => {
    let details: unknown = "leaf";
    for (let index = 0; index < 18; index += 1) details = [details];

    await expect(execute({ content: [], details })).rejects.toThrow(
      "workspace tool details are invalid",
    );
  });

  it.each([null, [], "schema"])("rejects malformed Pi schemas %#", (parameters) => {
    expect(() => adaptPiTool(piTool({ content: [] }, parameters), "low", [])).toThrow(
      "Pi tool schema is invalid",
    );
  });
});
