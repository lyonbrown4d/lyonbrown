use crate::Daemon;
use sha2::{Digest, Sha256};
use tonic::Status;

pub(crate) fn scope_record_id(scope: &str) -> String {
  let hash = hex::encode(Sha256::digest(scope.as_bytes()));
  format!("privacy-{}", &hash[..56])
}

impl Daemon {
  /// Store encrypted mappings with an owner fence before accepting content.
  pub(crate) async fn protect_private_text(
    &self,
    scope: &str,
    epoch: u64,
    text: &str,
  ) -> Result<String, Status> {
    let id = scope_record_id(scope);
    let mut privacy = self.privacy.lock().await;
    if let Some(ciphertext) = self
      .store
      .load_private_scope(scope)
      .await
      .map_err(|_| unavailable())?
    {
      let secrets = self.provider_secrets.clone();
      let record_id = id.clone();
      let mapping =
        tokio::task::spawn_blocking(move || secrets.open_value(&record_id, &ciphertext))
          .await
          .map_err(|_| unavailable())?
          .map_err(|_| unavailable())?;
      privacy
        .import_scope(scope, &mapping)
        .map_err(|_| unavailable())?;
    }
    let binding = self
      .store
      .model_binding(scope)
      .await
      .map_err(|_| unavailable())?;
    let daemon = self.clone();
    let active_secret = tokio::task::spawn_blocking(move || {
      if let Some(binding) = binding {
        daemon.binding_credential(&binding)
      } else if let Some(key) = &daemon.config.provider_api_key {
        Ok(Some(key.clone()))
      } else {
        let id = agent_config::provider_definition(&daemon.config.provider)
          .map_or(daemon.config.provider.as_str(), |definition| definition.id);
        use agent_secrets::ExposeSecret;
        Ok(
          daemon
            .provider_secrets
            .get(&format!("provider-{id}"))?
            .map(|secret| secret.expose_secret().to_owned()),
        )
      }
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?;
    if let Some(secret) = active_secret.as_deref().filter(|value| !value.is_empty()) {
      privacy
        .register_secret(scope, secret)
        .map_err(|_| unavailable())?;
    }
    let protected = privacy
      .protect_text(scope, text)
      .map_err(|_| unavailable())?;
    let mapping = privacy.export_scope(scope).map_err(|_| unavailable())?;
    let secrets = self.provider_secrets.clone();
    let ciphertext = tokio::task::spawn_blocking(move || secrets.seal_value(&id, &mapping))
      .await
      .map_err(|_| unavailable())?
      .map_err(|_| unavailable())?;
    self
      .store
      .commit_private_scope(scope, epoch, &ciphertext)
      .await
      .map_err(|_| unavailable())?;
    Ok(protected)
  }
}

fn unavailable() -> Status {
  Status::unavailable("privacy protection unavailable; input was not accepted")
}

impl Daemon {
  /// Register before publication; serialize replacement with prompt protection.
  pub(crate) async fn store_known_credential(
    &self,
    id: String,
    secret: agent_secrets::SecretString,
    preserve_replaced: bool,
  ) -> Result<(), Status> {
    let mut privacy = self.privacy.clone().lock_owned().await;
    privacy
      .register_known_secret(&secret)
      .map_err(|_| Status::failed_precondition("provider privacy protection unavailable"))?;
    let store = self.provider_secrets.clone();
    tokio::task::spawn_blocking(move || {
      let _privacy = privacy;
      use agent_secrets::ExposeSecret;
      if preserve_replaced
        && let Some(previous) = store.get(&id)?
        && previous.expose_secret() != secret.expose_secret()
      {
        archive_credential(&store, &previous)?;
      }
      store.put(&id, &secret)?;
      Ok::<_, agent_secrets::SecretError>(())
    })
    .await
    .map_err(|_| Status::failed_precondition("provider credential could not be stored"))?
    .map_err(|_| Status::failed_precondition("provider credential could not be stored"))?;
    Ok(())
  }

  pub(crate) async fn remove_known_credential(&self, id: String) -> Result<(), Status> {
    let privacy = self.privacy.clone().lock_owned().await;
    let store = self.provider_secrets.clone();
    tokio::task::spawn_blocking(move || {
      let _privacy = privacy;
      if let Some(previous) = store.get(&id)? {
        archive_credential(&store, &previous)?;
      }
      store.remove(&id)?;
      Ok::<_, agent_secrets::SecretError>(())
    })
    .await
    .map_err(|_| Status::failed_precondition("provider credential could not be removed"))?
    .map_err(|_| Status::failed_precondition("provider credential could not be removed"))?;
    Ok(())
  }
}

fn archive_credential(
  store: &agent_secrets::SecretStore,
  secret: &agent_secrets::SecretString,
) -> agent_secrets::Result<()> {
  use agent_secrets::ExposeSecret;
  const MAX_HISTORY: usize = 1024;
  let mut count = 0;
  for metadata in store.list()? {
    if !metadata.id.starts_with("privacy-history-") {
      continue;
    }
    count += 1;
    let previous = store
      .get(&metadata.id)?
      .ok_or(agent_secrets::SecretError::InvalidRecord)?;
    if previous.expose_secret() == secret.expose_secret() {
      return Ok(());
    }
  }
  if count >= MAX_HISTORY {
    return Err(agent_secrets::SecretError::InvalidSecret);
  }
  let id = format!("privacy-history-{}", uuid::Uuid::new_v4().simple());
  store.put(&id, secret)?;
  Ok(())
}
