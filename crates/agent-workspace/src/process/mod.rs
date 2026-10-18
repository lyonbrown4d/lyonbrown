//! Authorized, bounded native process execution.
//!
//! The workspace bounds the initial working directory only. Programs run with the current
//! user's OS rights; this is not a filesystem or network sandbox. An allowlisted program can
//! access other paths, launch programs, or deliberately escape a Unix process group. Only
//! trusted executables belong in the policy. No shell or ambient environment is inserted.
//! Canonical paths are checked again after approval. Callers must protect trusted paths
//! against concurrent replacement; path checks do not provide atomic filesystem isolation.
mod executor;
mod platform;
mod policy;
mod supervise;
mod types;

pub use executor::CommandExecutor;
pub use policy::CommandPolicy;
pub use types::*;
