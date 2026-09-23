//! MCP stdio servers are adapted into the ordinary tool registry.

mod bridge;
mod config;

pub use bridge::{McpBridge, McpError};
pub use config::McpServerConfig;
