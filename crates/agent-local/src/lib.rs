//! Security boundary primitives for discovering a local agent daemon.

mod auth;
mod descriptor;
mod error;
mod file_lock;
mod filesystem;
mod lifecycle_lock;
mod startup_lock;
mod token;
#[cfg(unix)]
mod unix_filesystem;
#[cfg(windows)]
mod windows_security;

pub use auth::{AUTHORIZATION_METADATA_KEY, AuthError, authorize_bearer, bearer_authorization};
pub use descriptor::{DiscoveryDescriptor, MAX_DISCOVERY_BYTES, ScopeFingerprint, Transport};
pub use error::DiscoveryError;
pub use filesystem::{DiscoveryStore, SecureRunDirectory};
pub use lifecycle_lock::LifecycleLock;
pub use startup_lock::StartupLock;
pub use token::SecretToken;
