//! Daemon-owned MCP connections exposed through the existing runtime tool port.
//! Server descriptions, annotations and returned resources are untrusted data.
mod catalog;
mod config;
mod connection;
mod http_client;
mod output;
mod stdio;
mod tool;

pub use config::{McpLimits, McpServerConfig, McpTransport, SecretHeaders, StdioConfig};
pub use tool::{McpApproval, McpAuthorization, McpManager};
