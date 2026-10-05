import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, test } from "vitest";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const protocolGeneratedRoot = resolve(repositoryRoot, "packages/protocol/src/generated");
const relativeJavaScriptSpecifier =
  /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)["'](\.{1,2}\/[^"']+[.](?:c|m)?js)["']/gu;

function listTypeScriptFiles(directory: string): string[] {
  if (!existsSync(directory)) {
    return [];
  }

  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      return listTypeScriptFiles(path);
    }
    return entry.isFile() && /[.]ts$/u.test(entry.name) ? [path] : [];
  });
}

function findJavaScriptSpecifiers(paths: readonly string[]): string[] {
  return paths.flatMap((path) => {
    const source = readFileSync(path, "utf8");
    return [...source.matchAll(relativeJavaScriptSpecifier)].map(
      (match) =>
        `${path.slice(repositoryRoot.length + 1)}: ${match[1] ?? "unknown relative JavaScript specifier"}`,
    );
  });
}

describe("TypeScript import policy", () => {
  test("uses TypeScript extensions for relative TypeScript source imports", () => {
    const trackedTypeScript = execFileSync(
      "git",
      ["ls-files", "--cached", "--others", "--exclude-standard", "--", "*.ts", "*.mts", "*.cts"],
      {
        cwd: repositoryRoot,
        encoding: "utf8",
      },
    )
      .split(/\r?\n/u)
      .filter((path) => path.length > 0);
    const violations = findJavaScriptSpecifiers(
      trackedTypeScript.map((path) => resolve(repositoryRoot, path)),
    );

    expect(violations).toEqual([]);
  });

  test("generates ignored TypeScript sources with TypeScript import extensions", () => {
    const codegenConfiguration = readFileSync(
      resolve(repositoryRoot, "packages/protocol/buf.gen.yaml"),
      "utf8",
    );
    const importSuffixes = [
      ...codegenConfiguration.matchAll(/^\s*- importSuffix=(\S+)\s*$/gmu),
    ].map((match) => match[1]);

    expect(importSuffixes).toEqual([".ts"]);
    expect(findJavaScriptSpecifiers(listTypeScriptFiles(protocolGeneratedRoot))).toEqual([]);
  });
});
