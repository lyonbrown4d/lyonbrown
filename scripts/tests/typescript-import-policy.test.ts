import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, test } from "vitest";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const protocolGeneratedRoot = resolve(repositoryRoot, "packages/protocol/src/generated");
const relativeModuleSpecifier = /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)["'](\.{1,2}\/[^"']+)["']/gu;
const privateWorkspaceSpecifier =
  /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)["'](@lyonbrown\/([^/"']+)\/(?:internal|test|script|generated)\/[^"']+)["']/gu;
const relativeJavaScriptSpecifier =
  /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)["'](\.{1,2}\/[^"']+[.](?:c|m)?js)["']/gu;
const workspaceRoots = new Map<string, string>([
  ["workspace", "scripts/"],
  ["agent-cli", "apps/agent-cli/"],
  ["agentd", "apps/agentd/"],
  ["agent-client", "packages/agent-client/"],
  ["agent-domain", "packages/agent-domain/"],
  ["agent-launch", "packages/agent-launch/"],
  ["agent-runtime", "packages/agent-runtime/"],
  ["control-plane-native", "packages/control-plane-native/"],
  ["decision-engine", "packages/decision-engine/"],
  ["event-store", "packages/event-store/"],
  ["kernel-pi", "packages/kernel-pi/"],
  ["local-control-plane", "packages/local-control-plane/"],
  ["local-model", "packages/local-model/"],
  ["protocol", "packages/protocol/"],
  ["ui-model", "packages/ui-model/"],
]);

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

function findRelativeModuleSpecifiers(paths: readonly string[]): string[] {
  return paths.flatMap((path) => {
    const source = readFileSync(path, "utf8");
    return [...source.matchAll(relativeModuleSpecifier)].map(
      (match) =>
        `${path.slice(repositoryRoot.length + 1)}: ${match[1] ?? "unknown relative module specifier"}`,
    );
  });
}

function findPrivateAliasEscapes(paths: readonly string[]): string[] {
  return paths.flatMap((path) => {
    const repositoryPath = path.slice(repositoryRoot.length + 1).replaceAll("\\", "/");
    const source = readFileSync(path, "utf8");
    return [...source.matchAll(privateWorkspaceSpecifier)].flatMap((match) => {
      const specifier = match[1] ?? "unknown private workspace specifier";
      const workspaceRoot = workspaceRoots.get(match[2] ?? "");
      return workspaceRoot !== undefined && repositoryPath.startsWith(workspaceRoot)
        ? []
        : [`${repositoryPath}: ${specifier}`];
    });
  });
}

function trackedTypeScriptFiles(): string[] {
  return execFileSync(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "--", "*.ts", "*.mts", "*.cts"],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
    },
  )
    .split(/\r?\n/u)
    .filter((path) => path.length > 0)
    .map((path) => resolve(repositoryRoot, path))
    .filter((path) => existsSync(path));
}

describe("TypeScript import policy", () => {
  test("rejects relative module specifiers in tracked TypeScript sources", () => {
    const trackedTypeScript = trackedTypeScriptFiles();
    const violations = findRelativeModuleSpecifiers(trackedTypeScript);

    expect(violations).toEqual([]);
  });

  test("keeps private aliases inside their owning workspace", () => {
    expect(findPrivateAliasEscapes(trackedTypeScriptFiles())).toEqual([]);
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
