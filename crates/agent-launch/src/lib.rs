//! Local daemon launch and reuse coordination.

mod command;
mod coordinator;
mod error;
mod grpc_probe;
mod local;
mod monitor;
mod paths;
mod ports;
mod resolver;
mod types;

pub use command::CommandDaemonSpawner;
pub use coordinator::LaunchCoordinator;
pub use error::{LaunchError, ProbeError, SpawnError};
pub use grpc_probe::GrpcReadinessProbe;
pub use local::LocalDiscoverySource;
pub use monitor::{DaemonExit, SpawnMonitor};
pub use paths::{canonicalize_optional_path, scoped_run_directory};
pub use ports::{DaemonSpawner, DiscoverySource, ReadinessProbe, StartupGuard};
pub use resolver::CoordinatorTargetResolver;
pub use types::{DaemonTarget, Handshake, LaunchRequest, SpawnRequest};
