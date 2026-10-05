export {
  DISCOVERY_SCHEMA_VERSION,
  DISCOVERY_TRANSPORT,
  MAX_DISCOVERY_BYTES,
  createDiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "./descriptor.ts";
export type { CreateDiscoveryDescriptorInput, DiscoveryDescriptor } from "./descriptor.ts";
export {
  StaleServerEpochError,
  bindDiscoveredEpoch,
  fenceEpochAtEnqueue,
  fenceEpochAtExecution,
} from "./epoch.ts";
export type { EpochBinding } from "./epoch.ts";
export { ControlPlaneError } from "./errors.ts";
export type {
  ControlPlaneErrorCode,
  ControlPlaneErrorDetails,
  ControlPlaneOperation,
} from "./errors.ts";
export { acquireLifecycleLock, acquireStartupLock } from "./locks.ts";
export type { DaemonLease } from "./locks.ts";
export type { NativeControlPlaneProvider } from "./platform-security.ts";
export { openSecureRunDirectory } from "./run-directory.ts";
export type { SecureRunDirectory } from "./run-directory.ts";
export { deriveScopedDaemonPaths } from "./paths.ts";
export type { ScopedDaemonPaths } from "./paths.ts";
export { deriveScope } from "./scope.ts";
export type { ScopeIdentity } from "./scope.ts";
export { SecretToken, verifySecretToken } from "./token.ts";
