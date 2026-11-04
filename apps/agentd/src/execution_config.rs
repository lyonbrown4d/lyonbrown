use crate::Daemon;
impl Daemon {
  pub(crate) async fn execution_config(
    &self,
    session: &agent_core::Session,
    skill_prompt: String,
    memory_context: Option<String>,
  ) -> anyhow::Result<agent_runtime::RuntimeConfig> {
    let mut runtime_config = self.config.runtime(session);
    if runtime_config.api_key.is_empty()
      && let Some(secret) = self
        .provider_secrets
        .get(&format!(
          "provider-{}",
          agent_config::provider_definition(&self.config.provider)
            .map_or(self.config.provider.as_str(), |entry| entry.id)
        ))
        .map_err(|_| anyhow::anyhow!("provider credential store unavailable"))?
    {
      use agent_secrets::ExposeSecret;
      runtime_config.api_key = secret.expose_secret().to_owned();
    }
    crate::repository_instructions::apply(&mut runtime_config, &session.workspace).await?;
    if !skill_prompt.is_empty() {
      runtime_config
        .system_prompt
        .get_or_insert_default()
        .push_str(&skill_prompt);
    }
    if let Some(memory_context) = memory_context {
      runtime_config
        .system_prompt
        .get_or_insert_default()
        .push_str(&memory_context);
    }
    self.normalize_runtime_limits(&mut runtime_config)?;
    Ok(runtime_config)
  }
}

impl Daemon {
  /// Inspect already loaded metadata; never load weights for a status request.
  pub(crate) fn normalize_runtime_limits(
    &self,
    config: &mut agent_runtime::RuntimeConfig,
  ) -> anyhow::Result<()> {
    if config.provider == "local" {
      let info = self.models.info(&config.model)?;
      config.context_window = info.context_window;
      config.max_output_tokens = self
        .config
        .local_models
        .iter()
        .find(|profile| profile.id == config.model)
        .map_or(512.min(info.context_window.saturating_sub(1)), |profile| {
          profile.max_output_tokens
        });
      config.supports_images = info.supports_images;
    }
    Ok(())
  }
}
