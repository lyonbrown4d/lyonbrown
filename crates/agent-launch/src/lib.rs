//! Per-user native service registration shared by CLI and Desktop.
#[cfg(unix)]
pub mod bundle_identity;
mod executable;
mod install;
pub mod model;
pub mod native;
mod options;
mod probe;
pub mod reconcile;
pub mod state;

use agent_client::AgentClient;
use anyhow::{Context, Result};

pub use executable::current_executable;
pub use options::LaunchOptions;

/// Register or reuse the same native service for CLI and Desktop.
pub async fn ensure_daemon(options: LaunchOptions) -> Result<AgentClient> {
  let candidate = options.candidate().await?;
  let runtime = tokio::runtime::Handle::current();
  let token_path = options.data_dir.join("auth.token");
  let token_for_probe = token_path.clone();
  tokio::task::spawn_blocking(move || {
    let lock = state::ServiceLock::for_current_user()?;
    let probe = probe::AuthenticatedProbe {
      token_path: token_for_probe,
    };
    runtime.block_on(reconcile::ensure_with_preparation(
      &native::NativeManager,
      &probe,
      &lock,
      candidate,
      &options.requirements,
      |candidate| install::retain_binary(&candidate),
    ))
  })
  .await
  .context("native service launcher failed")??;
  let token = tokio::fs::read_to_string(token_path)
    .await
    .context("read daemon auth.token")?;
  AgentClient::connect(model::LOCAL_ENDPOINT, &token).await
}
