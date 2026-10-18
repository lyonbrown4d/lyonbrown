//! Layered process configuration without mutating the process environment.
mod commands;
mod delegation;
mod load;
mod mcp;
mod models;
mod peer;
mod relay;
mod settings;

pub use commands::CommandSettings;
pub use delegation::{DelegationTargetSettings, validate_delegation_targets};
pub use load::{load, load_with_environment};
pub use mcp::{McpSettings, McpTransportSettings, validate_mcp};
pub use models::{LocalModelBackend, LocalModelFormat, LocalModelSettings, validate_local_models};
pub use peer::PeerSettings;
pub use relay::RelaySettings;
pub use settings::{DecisionSettings, Settings};

pub fn default_data_dir() -> anyhow::Result<std::path::PathBuf> {
  directories::ProjectDirs::from("org", "Lyonbrown", "Lyonbrown")
    .map(|dirs| dirs.data_local_dir().to_path_buf())
    .ok_or_else(|| anyhow::anyhow!("cannot locate user data directory"))
}
