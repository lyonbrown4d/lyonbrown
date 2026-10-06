export {
  DISCOVERY_SCHEMA_VERSION,
  DISCOVERY_TRANSPORT,
  MAX_DISCOVERY_BYTES,
  createDiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "@lyonbrown/local-control-plane/internal/descriptor";
export type {
  CreateDiscoveryDescriptorInput,
  DiscoveryDescriptor,
} from "@lyonbrown/local-control-plane/internal/descriptor";
export {
  StaleServerEpochError,
  bindDiscoveredEpoch,
  fenceEpochAtEnqueue,
  fenceEpochAtExecution,
} from "@lyonbrown/local-control-plane/internal/epoch";
export type { EpochBinding } from "@lyonbrown/local-control-plane/internal/epoch";
export { ControlPlaneError } from "@lyonbrown/local-control-plane/internal/errors";
export type {
  ControlPlaneErrorCode,
  ControlPlaneErrorDetails,
  ControlPlaneOperation,
} from "@lyonbrown/local-control-plane/internal/errors";
export {
  acquireLifecycleLock,
  acquireStartupLock,
} from "@lyonbrown/local-control-plane/internal/locks";
export type { DaemonLease } from "@lyonbrown/local-control-plane/internal/locks";
export type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane/internal/platform-security";
export { openSecureRunDirectory } from "@lyonbrown/local-control-plane/internal/run-directory";
export type { SecureRunDirectory } from "@lyonbrown/local-control-plane/internal/run-directory";
export { deriveScopedDaemonPaths } from "@lyonbrown/local-control-plane/internal/paths";
export type { ScopedDaemonPaths } from "@lyonbrown/local-control-plane/internal/paths";
export { deriveScope } from "@lyonbrown/local-control-plane/internal/scope";
export type { ScopeIdentity } from "@lyonbrown/local-control-plane/internal/scope";
export { SecretToken, verifySecretToken } from "@lyonbrown/local-control-plane/internal/token";
