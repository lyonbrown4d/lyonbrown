#!/usr/bin/env node

import { pathToFileURL } from "node:url";
import type { AgentLauncher } from "@lyonbrown/agent-launch";

import { runAgentTui } from "./app.js";
import { HELP_TEXT, VERSION, parseCliArgs } from "./cli.js";

export interface MainDependencies {
  readonly launcher: AgentLauncher;
}

export async function main(
  args: readonly string[],
  dependencies?: MainDependencies,
): Promise<void> {
  const mode = parseCliArgs(args);
  if (mode.kind === "help") {
    process.stdout.write(`${HELP_TEXT}\n`);
    return;
  }
  if (mode.kind === "version") {
    process.stdout.write(`${VERSION}\n`);
    return;
  }
  if (dependencies === undefined) {
    throw new Error("SECURE_AGENT_LAUNCHER_REQUIRED");
  }
  await runAgentTui(dependencies);
}

const entryUrl = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (import.meta.url === entryUrl) {
  void main(process.argv.slice(2)).catch((error: unknown) => {
    const code = error instanceof Error ? error.message : "AGENT_START_FAILED";
    process.stderr.write(`${code}\n`);
    process.exitCode = 1;
  });
}
