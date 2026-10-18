//! Self-hosted public directory and opaque relay; no model or session execution.
#![forbid(unsafe_code)]
mod relay;
mod service;

pub use service::ControlPlane;
