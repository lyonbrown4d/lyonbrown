export {
  DISCOVERY_SCHEMA_VERSION,
  DISCOVERY_TRANSPORT,
  MAX_DISCOVERY_BYTES,
  createDiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "./descriptor.js";
export type { CreateDiscoveryDescriptorInput, DiscoveryDescriptor } from "./descriptor.js";
export {
  StaleServerEpochError,
  bindDiscoveredEpoch,
  fenceEpochAtEnqueue,
  fenceEpochAtExecution,
} from "./epoch.js";
export type { EpochBinding } from "./epoch.js";
export { ControlPlaneError } from "./errors.js";
export type {
  ControlPlaneErrorCode,
  ControlPlaneErrorDetails,
  ControlPlaneOperation,
} from "./errors.js";
export { acquireLifecycleLock, acquireStartupLock } from "./locks.js";
export type { DaemonLease } from "./locks.js";
export type { NativeControlPlaneProvider } from "./platform-security.js";
export { openSecureRunDirectory } from "./run-directory.js";
export type { SecureRunDirectory } from "./run-directory.js";
export { deriveScopedDaemonPaths } from "./paths.js";
export type { ScopedDaemonPaths } from "./paths.js";
export { deriveScope } from "./scope.js";
export type { ScopeIdentity } from "./scope.js";
export { SecretToken, verifySecretToken } from "./token.js";
