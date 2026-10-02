//! Loopback-only gRPC boundary for a shared Lyonbrown agent runtime.

pub mod args;
mod auth;
pub mod epoch;
mod host;
mod mapping;
pub mod model_setup;
mod replay;
pub mod runtime;
#[doc(hidden)]
pub mod runtime_cleanup;
mod server;
mod service;

pub use host::{
  AppControllerHost, CAPABILITY_MODEL_SETUP_DOWNLOAD_V1, ManagedRuntimeHost, RuntimeHost,
  VersionedSnapshot,
};
pub use mapping::snapshot_to_wire;
pub use server::{ServeError, serve_authenticated};
