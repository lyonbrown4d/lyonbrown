use super::*;
impl Daemon {
  pub(super) async fn context_binding(
    &self,
    id: &str,
  ) -> Result<(Self, agent_core::Session, String), Status> {
    let session = self
      .store
      .session(id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::failed_precondition(
        "session belongs to another owner",
      ));
    }
    let latest = self
      .store
      .runs(id)
      .await
      .map_err(db_error)?
      .into_iter()
      .max_by_key(|run| run.created_at_ms);
    let run_id = latest
      .as_ref()
      .map_or_else(|| session.id.clone(), |run| run.id.clone());
    let configured = self
      .configured_for_run(&session.id)
      .await
      .map_err(db_error)?;
    Ok((configured, session, run_id))
  }
  pub(super) fn context_persistence(&self, run_id: String) -> Arc<dyn SessionPersistence> {
    Arc::new(RunPersistence {
      store: self.store.clone(),
      run_id,
    })
  }
  pub(super) async fn context_runtime(
    &self,
    mut config: RuntimeConfig,
    run_id: String,
  ) -> Result<Runtime, Status> {
    if config.api_key.is_empty() {
      use agent_secrets::ExposeSecret;
      if let Some(secret) = self
        .provider_secrets
        .get(&format!(
          "provider-{}",
          agent_config::provider_definition(&config.provider)
            .map_or(config.provider.as_str(), |p| p.id)
        ))
        .map_err(|_| Status::unavailable("provider credential store unavailable"))?
      {
        config.api_key = secret.expose_secret().to_owned();
      }
    }
    let listener = Arc::new(|_| {});
    let persistence = self.context_persistence(run_id);
    if config.provider == "local" {
      let backend = self.models.backend(&config.model).await.map_err(db_error)?;
      Runtime::open_with_backend(config, persistence, vec![], listener, backend)
        .await
        .map_err(runtime_error)
    } else {
      Runtime::open(config, persistence, vec![], listener)
        .await
        .map_err(runtime_error)
    }
  }
  pub(super) fn context_config(
    &self,
    session: &agent_core::Session,
  ) -> Result<RuntimeConfig, Status> {
    let mut config = self.config.runtime(session);
    self
      .normalize_runtime_limits(&mut config)
      .map_err(db_error)?;
    Ok(config)
  }
}
