//! Loopback-only gRPC boundary for a shared Lyonbrown agent runtime.

mod host;
mod mapping;
mod replay;
mod server;
mod service;

pub use host::{AppControllerHost, RuntimeHost, VersionedSnapshot};
pub use mapping::snapshot_to_wire;
pub use server::{ServeError, serve};
pub use service::AgentGrpcService;
