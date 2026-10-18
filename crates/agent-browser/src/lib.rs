//! Browser facade and private daemon-to-host IPC; CEF types remain in the host.
mod client;
mod diagnostics;
mod environment;
mod model;
mod profile;
mod validation;
pub mod wire;

pub use client::BrowserHost;
pub use environment::restricted_environment;
pub use model::*;
pub use profile::ProfileLease;
pub use validation::{validate_action, validate_url};
