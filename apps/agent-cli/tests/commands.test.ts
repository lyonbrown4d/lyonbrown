import { describe, expect, it } from "vitest";

import { parseInput } from "../src/commands.ts";

describe("TUI commands", () => {
  it("keeps ordinary multilingual text as chat input", () => {
    expect(parseInput("请解释这个项目")).toEqual({ kind: "chat", text: "请解释这个项目" });
  });

  it("parses control commands without accepting empty remote payloads", () => {
    expect(parseInput("/language en")).toEqual({ kind: "language", locale: "en-US" });
    expect(parseInput("/steer focus tests")).toEqual({ kind: "steer", text: "focus tests" });
    expect(() => parseInput("/steer   ")).toThrow(/requires text/u);
  });

  it("rejects unknown commands instead of sending them to the model", () => {
    expect(() => parseInput("/unknown secret")).toThrow(/unknown command/u);
  });
});
