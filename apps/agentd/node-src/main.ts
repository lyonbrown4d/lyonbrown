import { pathToFileURL } from "node:url";
import { ControlPlaneError } from "@lyonbrown/local-control-plane";
import { startProductionAgentd } from "./bootstrap.js";

export async function main(): Promise<void> {
  await startProductionAgentd({
    workspace: process.cwd(),
    effectiveConfigFingerprint: "production-default",
  });
}

const entryUrl = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entryUrl === import.meta.url) {
  void main().catch((error: unknown) => {
    const code = error instanceof ControlPlaneError ? error.code : "AGENTD_START_FAILED";
    process.stderr.write(`${code}\n`);
    process.exitCode = 1;
  });
}
