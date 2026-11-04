use crate::Daemon;
use agent_storage::ModelBinding;

impl Daemon {
  pub(crate) async fn model_bindings(
    &self,
  ) -> anyhow::Result<Vec<(String, String, String, ModelBinding)>> {
    let mut entries = vec![(
      self.config.model.clone(),
      self.config.model.clone(),
      self.config.provider.clone(),
      ModelBinding {
        model_id: self.config.model.clone(),
        execution_id: "configured".into(),
        provider: self.config.provider.clone(),
        model: self.config.model.clone(),
        endpoint: self.config.api_base.clone(),
        credential_id: agent_config::provider_definition(&self.config.provider)
          .map_or(self.config.provider.as_str(), |entry| entry.id)
          .into(),
      },
    )];
    for id in self
      .config
      .gateway
      .models
      .iter()
      .map(|model| &model.id)
      .chain(self.config.gateway.groups.iter().map(|group| &group.id))
    {
      entries.push((
        id.clone(),
        id.clone(),
        "Gateway".into(),
        ModelBinding {
          model_id: id.clone(),
          execution_id: format!("gateway:{id}"),
          provider: "openai-completions".into(),
          model: id.clone(),
          endpoint: None,
          credential_id: String::new(),
        },
      ));
    }
    for (id, name, vendor, protocol, endpoint) in [
      (
        "deepseek-chat",
        "DeepSeek Chat",
        "deepseek",
        "deepseek",
        "https://api.deepseek.com",
      ),
      (
        "deepseek-reasoner",
        "DeepSeek Reasoner",
        "deepseek",
        "deepseek",
        "https://api.deepseek.com",
      ),
      (
        "gemini-2.5-flash",
        "Gemini 2.5 Flash",
        "gemini",
        "gemini",
        "https://generativelanguage.googleapis.com",
      ),
      (
        "openrouter/auto",
        "OpenRouter Auto",
        "openrouter",
        "openrouter",
        "https://openrouter.ai/api/v1",
      ),
      (
        "llama-3.3-70b-versatile",
        "Groq Llama 3.3 70B",
        "groq",
        "groq",
        "https://api.groq.com/openai/v1",
      ),
      (
        "mistral-small-latest",
        "Mistral Small",
        "mistral",
        "mistral",
        "https://api.mistral.ai/v1",
      ),
      (
        "gpt-4.1",
        "GPT-4.1",
        "openai",
        "openai-responses",
        "https://api.openai.com/v1",
      ),
      (
        "claude-sonnet-4-5",
        "Claude Sonnet 4.5",
        "anthropic",
        "anthropic",
        "https://api.anthropic.com",
      ),
    ] {
      if entries.iter().any(|entry| entry.0 == id) {
        continue;
      }
      entries.push((
        id.into(),
        name.into(),
        vendor.into(),
        ModelBinding {
          model_id: id.into(),
          execution_id: format!("builtin:{id}"),
          provider: protocol.into(),
          model: id.into(),
          endpoint: Some(endpoint.into()),
          credential_id: vendor.into(),
        },
      ));
    }
    for profile in &self.config.local_models {
      if entries.iter().any(|entry| entry.0 == profile.id) {
        continue;
      }
      entries.push((
        profile.id.clone(),
        profile.id.clone(),
        "Imported".into(),
        ModelBinding {
          model_id: profile.id.clone(),
          execution_id: format!("local:{}", profile.id),
          provider: "local".into(),
          model: profile.id.clone(),
          endpoint: None,
          credential_id: String::new(),
        },
      ));
    }
    for info in self.models.all() {
      if entries.iter().any(|entry| entry.0 == info.id) {
        continue;
      }
      entries.push((
        info.id.clone(),
        info.id.clone(),
        "Hugging Face".into(),
        ModelBinding {
          model_id: info.id.clone(),
          execution_id: format!("local:{}", info.id),
          provider: "local".into(),
          model: info.id,
          endpoint: None,
          credential_id: String::new(),
        },
      ));
    }
    for profile in self.store.provider_profiles().await? {
      let runtime_provider = profile.runtime_provider();
      let id = format!("profile:{}", profile.id);
      entries.push((
        id.clone(),
        profile.name.clone(),
        profile.name,
        ModelBinding {
          model_id: id,
          execution_id: format!("profile:{}:{}", profile.id, profile.revision),
          provider: runtime_provider.into(),
          model: profile.model,
          endpoint: Some(profile.base_url),
          credential_id: format!("profile-secret:{}", profile.credential_id),
        },
      ));
    }
    Ok(entries)
  }
  pub(crate) fn binding_credential(
    &self,
    binding: &ModelBinding,
  ) -> anyhow::Result<Option<String>> {
    use agent_secrets::ExposeSecret;
    if let Some(id) = binding.credential_id.strip_prefix("profile-secret:") {
      return Ok(
        self
          .provider_secrets
          .get(id)?
          .map(|secret| secret.expose_secret().to_owned()),
      );
    }
    if let Some(value) = self.config.catalog_credentials.get(&binding.credential_id) {
      return Ok(Some(value.clone()));
    }
    if binding.execution_id == "configured"
      && let Some(key) = &self.config.provider_api_key
      && !key.is_empty()
    {
      return Ok(Some(key.clone()));
    }
    Ok(
      self
        .provider_secrets
        .get(&format!("provider-{}", binding.credential_id))?
        .map(|secret| secret.expose_secret().to_owned()),
    )
  }
  pub(crate) async fn configured_for_run(&self, run: &str) -> anyhow::Result<Self> {
    let Some(binding) = self.store.model_binding(run).await? else {
      return Ok(self.clone());
    };
    let mut daemon = self.clone();
    let mut config = self.config.clone();
    config.gateway_registry = Some(self.uses_gateway_registry(&binding));
    config.provider = binding.provider.clone();
    config.model = binding.model.clone();
    config.api_base = binding.endpoint.clone();
    config.provider_api_key = self.binding_credential(&binding)?;
    daemon.config = config;
    Ok(daemon)
  }
}
