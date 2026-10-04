// @ts-expect-error Composition options are intentionally not part of the public client entry.
export type { CreateAgentLauncherOptions } from "../../src/index.js";
// @ts-expect-error Native discovery context is intentionally not public.
export type { DiscoveryContext } from "../../src/index.js";
// @ts-expect-error Composition ports are intentionally not public.
export type { AgentLaunchPorts } from "../../src/index.js";
// @ts-expect-error Process spawning is intentionally not public.
export type { ProcessSpawner } from "../../src/index.js";
// @ts-expect-error Client factory composition is intentionally not public.
export type { AgentClientFactory } from "../../src/index.js";
// @ts-expect-error Launcher construction belongs to the private composition root.
export { createAgentLauncher } from "../../src/index.js";
