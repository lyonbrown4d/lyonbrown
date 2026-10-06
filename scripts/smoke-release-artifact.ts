import { constants } from "node:fs";
import { access, readdir, readFile, realpath } from "node:fs/promises";
import { resolve, join } from "node:path";

import { $, usePowerShell } from "zx";

import { configureZxPlatform } from "./zx-platform.ts";

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
  "const llama=await module.getLlama({gpu:false,build:'never',skipDownload:true,usePrebuiltBinaries:true,progressLogs:false,dryRun:true});",
  "await llama.dispose();",
  "process.stdout.write('LLAMA_ARTIFACT_OK');",
].join("");

function artifactArgument(arguments_: readonly string[]): string {
  if (arguments_.length !== 2 || arguments_[0] !== "--artifact" || arguments_[1] === undefined) {
    throw new Error("Usage: smoke-release-artifact --artifact <directory>");
  }
  return resolve(arguments_[1]);
}

async function assertNoFilesystemLinks(directory: string): Promise<void> {
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isSymbolicLink()) {
      throw new Error(`Release artifact contains a filesystem link: ${path}`);
    }
    if (entry.isDirectory()) await assertNoFilesystemLinks(path);
  }
}

async function main(): Promise<void> {
  const artifact = await realpath(artifactArgument(process.argv.slice(2)));
  const runtime = join(artifact, "runtime", "node.exe");
  const entry = join(artifact, "dist", "main.js");
  await Promise.all([
    access(runtime, constants.X_OK),
    access(entry, constants.R_OK),
    access(join(artifact, "agent.cmd"), constants.R_OK),
  ]);
  await assertNoFilesystemLinks(artifact);

  const manifest = JSON.parse(await readFile(join(artifact, "package.json"), "utf8")) as {
    readonly name?: unknown;
    readonly version?: unknown;
  };
  if (manifest.name !== "@lyonbrown/agent-cli" || typeof manifest.version !== "string") {
    throw new Error("Release artifact manifest is invalid");
  }

  const version = (
    await $({ cwd: artifact, quiet: true })`& ${runtime} ${entry} --version`
  ).stdout.trim();
  if (version !== manifest.version) throw new Error("Release artifact version does not match");

  await $({ cwd: artifact, quiet: true })`& ${runtime} --input-type=module --eval ${IMPORT_SMOKE}`;
  await $({ cwd: artifact, quiet: true })`& ${runtime} --input-type=module --eval ${SQLITE_SMOKE}`;
  await $({ cwd: artifact, quiet: true })`& ${runtime} --input-type=module --eval ${LLAMA_SMOKE}`;
  console.log(`RELEASE_ARTIFACT_SMOKE_OK ${manifest.version}`);
}

try {
  await main();
} catch (error: unknown) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
