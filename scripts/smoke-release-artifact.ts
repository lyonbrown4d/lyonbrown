import { constants } from "node:fs";
import { access, readFile, realpath } from "node:fs/promises";
import { join } from "node:path";

import { $, usePowerShell } from "zx";

import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";
import { assertPortableArtifactFiles } from "@lyonbrown/workspace/internal/release-artifact-files";
import {
  releaseRuntimeName,
  releaseLauncherName,
  resolveArtifactSmokeDirectory,
} from "@lyonbrown/workspace/internal/release-artifact-lib";

configureZxPlatform(process.platform, usePowerShell);

const IMPORT_SMOKE =
  "await import('@lyonbrown/agentd/daemon'); process.stdout.write('AGENTD_ARTIFACT_OK');";
const SQLITE_SMOKE = [
  "import { realpathSync } from 'node:fs';",
  "import { createRequire } from 'node:module';",
  "import { dirname, resolve } from 'node:path';",
  "const rootRequire=createRequire(import.meta.url);",
  "const entry=realpathSync(rootRequire.resolve('@lyonbrown/agentd/daemon'));",
  "const scope=resolve(dirname(entry),'..','..');",
  "const storeEntry=realpathSync(resolve(scope,'event-store','dist','index.js'));",
  "const storeRequire=createRequire(storeEntry);",
  "const Database=storeRequire('better-sqlite3');",
  "const db=new Database(':memory:');",
  "db.pragma('journal_mode');",
  "db.close();",
  "process.stdout.write('SQLITE_ARTIFACT_OK');",
].join("");
const LLAMA_SMOKE = [
  "import { realpathSync } from 'node:fs';",
  "import { createRequire } from 'node:module';",
  "import { dirname, resolve } from 'node:path';",
  "import { pathToFileURL } from 'node:url';",
  "const rootRequire=createRequire(import.meta.url);",
  "const entry=realpathSync(rootRequire.resolve('@lyonbrown/agentd/daemon'));",
  "const scope=resolve(dirname(entry),'..','..');",
  "const modelEntry=realpathSync(resolve(scope,'local-model','dist','index.js'));",
  "const modelRequire=createRequire(modelEntry);",
  "const module=await import(pathToFileURL(modelRequire.resolve('node-llama-cpp')).href);",
  // dryRun already disposes the returned instance inside node-llama-cpp.
  "await module.getLlama({gpu:process.platform==='darwin'?'auto':false,build:'never',skipDownload:true,usePrebuiltBinaries:true,progressLogs:false,dryRun:true});",
  "process.stdout.write('LLAMA_ARTIFACT_OK');",
].join("");

async function main(): Promise<void> {
  const artifact = await realpath(resolveArtifactSmokeDirectory(process.argv.slice(2)));
  const runtime = join(artifact, "runtime", releaseRuntimeName(process.platform));
  const entry = join(artifact, "dist", "main.js");
  await Promise.all([
    access(runtime, constants.X_OK),
    access(entry, constants.R_OK),
    access(join(artifact, releaseLauncherName(process.platform)), constants.R_OK),
  ]);
  await assertPortableArtifactFiles(artifact);

  const manifest = JSON.parse(await readFile(join(artifact, "package.json"), "utf8")) as {
    readonly name?: unknown;
    readonly version?: unknown;
  };
  if (manifest.name !== "@lyonbrown/agent-cli" || typeof manifest.version !== "string") {
    throw new Error("Release artifact manifest is invalid");
  }

  // zx argument arrays preserve argv on both PowerShell and POSIX shells.
  const invoke = (...args: readonly string[]) => {
    const command = [runtime, ...args];
    return process.platform === "win32"
      ? $({ cwd: artifact, quiet: true, timeout: 60_000 })`& ${command}`
      : $({ cwd: artifact, quiet: true, timeout: 60_000 })`${command}`;
  };
  const version = (await invoke(entry, "--version")).stdout.trim();
  if (version !== manifest.version) throw new Error("Release artifact version does not match");

  await invoke("--input-type=module", "--eval", IMPORT_SMOKE);
  await invoke("--input-type=module", "--eval", SQLITE_SMOKE);
  await invoke("--input-type=module", "--eval", LLAMA_SMOKE);
  console.log(`RELEASE_ARTIFACT_SMOKE_OK ${manifest.version}`);
}

try {
  await main();
} catch (error: unknown) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
