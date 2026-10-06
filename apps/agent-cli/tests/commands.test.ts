import { describe, expect, it } from "vitest";

import { parseInput } from "@lyonbrown/agent-cli/internal/commands";

describe("TUI commands", () => {
  it("keeps ordinary multilingual text as chat input", () => {
    expect(parseInput("请解释这个项目")).toEqual({ kind: "chat", text: "请解释这个项目" });
  });

  it("parses control commands without accepting empty remote payloads", () => {
    expect(parseInput("/language en")).toEqual({ kind: "language", locale: "en-US" });
    expect(parseInput("/steer focus tests")).toEqual({ kind: "steer", text: "focus tests" });
    expect(() => parseInput("/steer   ")).toThrow(/requires text/u);
  });

  it("parses the accepted control, queue, status and exit command surface", () => {
    expect(parseInput("/model")).toEqual({ kind: "control", tab: "model" });
    expect(parseInput("/model auto")).toEqual({ kind: "model", selection: { kind: "auto" } });
    expect(parseInput("/model openai/gpt-5")).toEqual({
      kind: "model",
      selection: { kind: "pinned", provider: "openai", model: "gpt-5" },
    });
    expect(parseInput("/theme")).toEqual({ kind: "control", tab: "theme" });
    expect(parseInput("/language")).toEqual({ kind: "control", tab: "language" });
    expect(parseInput("/queue review the tests")).toEqual({
      kind: "follow-up",
      text: "review the tests",
    });
    expect(parseInput("/status")).toEqual({ kind: "status" });
    expect(parseInput("/sessions")).toEqual({ kind: "sessions" });
    expect(parseInput("/quit")).toEqual({ kind: "quit" });
  });

  it("rejects unknown commands instead of sending them to the model", () => {
    expect(() => parseInput("/unknown secret")).toThrow(/unknown command/u);
  });
});
