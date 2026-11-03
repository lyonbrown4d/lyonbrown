//! Startup registration completes before the Gateway or daemon is published.
use std::sync::Arc;

use agent_secrets::{SecretError, SecretStore};
use anyhow::{Context, Result};
use tokio::sync::Mutex;

pub(super) async fn register_stored_credentials(
  store: Arc<SecretStore>,
  privacy: Arc<Mutex<agent_privacy::PrivacyService>>,
) -> Result<()> {
  let stored = tokio::task::spawn_blocking(move || {
    store
      .list()?
      .into_iter()
      .map(|metadata| store.get(&metadata.id)?.ok_or(SecretError::InvalidRecord))
      .collect::<agent_secrets::Result<Vec<_>>>()
  })
  .await
  .context("credential inventory worker failed")??;
  let mut privacy = privacy.lock().await;
  for secret in &stored {
    privacy
      .register_known_secret(secret)
      .map_err(|_| anyhow::anyhow!("privacy credential registration failed"))?;
  }
  Ok(())
}
