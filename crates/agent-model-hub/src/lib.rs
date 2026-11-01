//! Bounded model discovery and artifact installation; inference remains agentd-owned.
mod client;
mod files;
mod install;
mod metadata;
mod plan;
mod transfer;
mod types;

pub use client::HubClient;
pub use plan::plan;
pub use types::*;
