// @ts-expect-error Composition options are intentionally not part of the public client entry.
export type { CreateAgentLauncherOptions } from "@lyonbrown/agent-launch";
// @ts-expect-error Native discovery context is intentionally not public.
export type { DiscoveryContext } from "@lyonbrown/agent-launch";
// @ts-expect-error Composition ports are intentionally not public.
export type { AgentLaunchPorts } from "@lyonbrown/agent-launch";
// @ts-expect-error Process spawning is intentionally not public.
export type { ProcessSpawner } from "@lyonbrown/agent-launch";
// @ts-expect-error Client factory composition is intentionally not public.
export type { AgentClientFactory } from "@lyonbrown/agent-launch";
// @ts-expect-error Launcher construction belongs to the private composition root.
export { createAgentLauncher } from "@lyonbrown/agent-launch";

export { createAgentLauncher as createForComposition } from "@lyonbrown/agent-launch/composition";
export type {
  AgentLaunchPorts as CompositionPorts,
  NativeControlPlaneProvider as CompositionNativeProvider,
  ProcessSpawner as CompositionSpawner,
} from "@lyonbrown/agent-launch/composition";
