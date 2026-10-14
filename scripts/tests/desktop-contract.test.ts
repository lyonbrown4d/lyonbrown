import { existsSync, readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

describe("redesigned desktop foundation", () => {
  test("registers an independent workspace and moon development command", () => {
    expect(existsSync("apps/agent-desktop/package.json")).toBe(true);
    expect(readFileSync(".moon/workspace.yml", "utf8")).toContain(
      'agent-desktop: ".moon/projects/agent-desktop"',
    );
    const root = JSON.parse(readFileSync("package.json", "utf8")) as {
      scripts: Record<string, string>;
    };
    expect(root.scripts["dev:desktop"]).toBe("moon run agent-desktop:dev");
  });

  test("keeps backend ownership outside the sandboxed renderer", () => {
    expect(existsSync("apps/agent-desktop/src/main/production.ts")).toBe(true);
    const production = readFileSync("apps/agent-desktop/src/main/production.ts", "utf8");
    expect(production).toContain("@lyonbrown/agent-launch/composition");
    expect(production).not.toContain("process.execPath");
    const main = readFileSync("apps/agent-desktop/src/main/index.ts", "utf8");
    expect(main).toContain("sandbox: true");
    expect(main).toContain("contextIsolation: true");
    expect(main).toContain("nodeIntegration: false");
  });
});
