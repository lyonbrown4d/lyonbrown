import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

import Database from "better-sqlite3";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);

function workspaceList(contents: string, key: string): readonly string[] {
  const lines = contents.split(/\r?\n/u);
  const start = lines.findIndex((line) => line === `${key}:`);
  if (start < 0) return [];

  const entries: string[] = [];
  for (const line of lines.slice(start + 1)) {
    const match = /^\s{2}-\s+["']?([^"']+?)["']?\s*$/u.exec(line);
    if (match?.[1] !== undefined) {
      entries.push(match[1]);
      continue;
    }
    if (line.trim() !== "" && !line.startsWith(" ")) break;
  }
  return entries;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function prebuildPlatform(): string {
  if (process.platform !== "linux") return process.platform;
  const report: unknown = process.report.getReport();
  const header = isRecord(report) ? report.header : undefined;
  const glibcVersion = isRecord(header) ? header.glibcVersionRuntime : undefined;
  return typeof glibcVersion === "string" && glibcVersion.length > 0 ? "linux" : "linuxmusl";
}

describe("better-sqlite3 native packaging policy", () => {
  it("does not run better-sqlite3's implicit node-gyp build", () => {
    const workspace = readFileSync(
      new URL("../../../pnpm-workspace.yaml", import.meta.url),
      "utf8",
    );

    expect(workspaceList(workspace, "onlyBuiltDependencies")).toEqual(["esbuild"]);
    expect(workspaceList(workspace, "ignoredBuiltDependencies")).toContain("better-sqlite3");
  });

  it("ships and loads the current platform prebuild with WAL support", () => {
    const packageRoot = dirname(require.resolve("better-sqlite3/package.json"));
    const prebuild = join(packageRoot, "prebuilds", `${prebuildPlatform()}-${process.arch}.node`);
    expect(existsSync(prebuild), `Missing packaged SQLite prebuild: ${prebuild}`).toBe(true);

    const directory = mkdtempSync(join(tmpdir(), "lyonbrown-sqlite-prebuild-"));
    const database = new Database(join(directory, "smoke.sqlite3"));
    try {
      expect(database.pragma("journal_mode = WAL", { simple: true })).toBe("wal");
    } finally {
      database.close();
      rmSync(directory, { force: true, recursive: true });
    }
  });
});
