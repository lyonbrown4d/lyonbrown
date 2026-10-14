import { homedir } from "node:os";
import { resolve } from "node:path";
import { config as loadEnvironment } from "dotenv";
import { loadDesktopConfiguration } from "@lyonbrown/agent-desktop/internal/main/configuration";
const repositoryRoot = resolve(import.meta.dirname, "../../../..");
export function runtimeConfiguration() {
  loadEnvironment({ path: resolve(repositoryRoot, ".env.local"), quiet: true, override: false });
  loadEnvironment({ path: resolve(repositoryRoot, ".env"), quiet: true, override: false });
  return loadDesktopConfiguration(process.env, {
    workspace: repositoryRoot,
    homeDirectory: homedir(),
  });
}
export function rendererEnvironmentUrl(): string | undefined {
  return process.env.ELECTRON_RENDERER_URL;
}
export function workerEnvironment(): NodeJS.ProcessEnv {
  return { ...process.env };
}
