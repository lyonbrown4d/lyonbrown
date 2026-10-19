//! Synthetic, loopback-only model endpoints. No paid calls or native model weights.
#![forbid(unsafe_code)]
mod config;
mod finite;
mod ollama;
mod scenario;
mod server;
pub use config::{AppConfig, Cli};
pub use scenario::{Protocol, Reply, Scenario};
pub use server::MockApp;
