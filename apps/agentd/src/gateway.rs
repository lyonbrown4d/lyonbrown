//! Composition of the in-process model gateway and its durable admission store.
use std::{
  collections::{BTreeSet, HashMap},
  sync::{Arc, Mutex},
};

use agent_secrets::{ExposeSecret, SecretStore, SecretString};
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use decision_core::{DecisionEngine, ModelTier};
use gateway_core::{
  AccessControl, AccessError, AccessPrincipal, Gateway, GatewayConfig, ModelConfig, ModelGroup,
  ProviderEndpoint, Secret,
};
use gateway_storage::GatewayStorage;

mod cache;
mod known;
use cache::cache_key;

use crate::config::Config;

pub(crate) struct GatewayHost {
  pub(crate) gateway: Option<Gateway>,
  pub(crate) internal_token: SecretString,
  explicit: bool,
  decision: Arc<DecisionEngine>,
  storage: GatewayStorage,
  access: Arc<CurrentAccess>,
  cache: Mutex<HashMap<[u8; 32], Gateway>>,
  privacy: Arc<tokio::sync::Mutex<agent_privacy::PrivacyService>>,
}

struct CurrentAccess {
  storage: GatewayStorage,
  allowed_ids: BTreeSet<String>,
}

#[async_trait]
impl AccessControl for CurrentAccess {
  async fn authorize(&self, bearer: &str) -> Result<bool, AccessError> {
    Ok(self.authenticate(bearer).await?.is_some())
  }

  async fn authenticate(&self, bearer: &str) -> Result<Option<AccessPrincipal>, AccessError> {
    Ok(
      self
        .storage
        .authenticate(bearer)
        .await?
        .filter(|principal| self.allowed_ids.contains(&principal.key_id)),
    )
  }
}

pub(crate) async fn build(
  config: &Config,
  decision: Arc<DecisionEngine>,
  provider_secrets: Arc<SecretStore>,
  privacy: Arc<tokio::sync::Mutex<agent_privacy::PrivacyService>>,
) -> Result<GatewayHost> {
  config.gateway.validate()?;
  let token_store = provider_secrets.clone();
  let internal_token = tokio::task::spawn_blocking(move || -> Result<SecretString> {
    const ID: &str = "gateway-internal-token";
    if let Some(token) = token_store.get(ID)? {
      return Ok(token);
    }
    let token: SecretString = format!(
      "{}{}",
      uuid::Uuid::new_v4().simple(),
      uuid::Uuid::new_v4().simple()
    )
    .into();
    token_store.put(ID, &token)?;
    Ok(token)
  })
  .await
  .context("gateway credential worker failed")??;

  known::register_stored_credentials(provider_secrets.clone(), privacy.clone()).await?;

  let mut models = config
    .gateway
    .models
    .iter()
    .map(|model| {
      Ok(ModelConfig {
        id: model.id.clone(),
        upstream_model: model.upstream_model.clone(),
        api_base: model.api_base.clone(),
        api_key: credential(config, model.api_key_env.as_deref())?,
        api_key_env: model.api_key_env.clone(),
        tier: ModelTier::Balanced,
        local_inference: false,
        capabilities: model.capabilities.clone(),
        context_tokens: Some(model.context_tokens),
      })
    })
    .collect::<Result<Vec<_>>>()?;
  if models.is_empty() && config.provider == "openai-completions" {
    let key = match &config.provider_api_key {
      Some(key) => Some(Secret::new(key.clone())),
      None => {
        let store = provider_secrets.clone();
        tokio::task::spawn_blocking(move || store.get("provider-openai"))
          .await
          .context("provider credential worker failed")??
          .map(|value| Secret::new(value.expose_secret().to_owned()))
      }
    };
    models.push(ModelConfig {
      id: config.model.clone(),
      upstream_model: config.model.clone(),
      api_base: config
        .api_base
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into()),
      api_key: key,
      api_key_env: None,
      tier: ModelTier::Balanced,
      local_inference: false,
      capabilities: ["text".to_owned(), "tools".to_owned()]
        .into_iter()
        .collect(),
      context_tokens: Some(128_000),
    });
  }
  ensure!(
    !models.is_empty() || config.gateway.http_listen.is_none(),
    "gateway HTTP requires a Chat Completions model"
  );
  let endpoints = config
    .gateway
    .endpoints
    .iter()
    .map(|endpoint| {
      Ok(ProviderEndpoint {
        id: endpoint.id.clone(),
        provider_id: endpoint.provider_id.clone(),
        api_base: endpoint.api_base.clone(),
        api_key: credential(config, endpoint.api_key_env.as_deref())?,
        api_key_env: endpoint.api_key_env.clone(),
        enabled: true,
        priority: endpoint.priority,
        max_concurrent: 1,
        model_ids: endpoint.model_ids.clone(),
      })
    })
    .collect::<Result<Vec<_>>>()?;
  let groups = config
    .gateway
    .groups
    .iter()
    .map(|group| ModelGroup {
      id: group.id.clone(),
      models: group.models.clone(),
    })
    .collect();
  let gateway_config = GatewayConfig {
    token: Secret::new(internal_token.expose_secret().to_owned()),
    models,
    endpoints,
    groups,
    limits: Default::default(),
    constraints: Default::default(),
  };
  if !gateway_config.models.is_empty() {
    gateway_config.validate()?;
  }
  let storage = GatewayStorage::open(config.data_path()?.join("gateway.sqlite"))
    .await
    .context("open gateway audit storage")?;
  let mut allowed_ids = BTreeSet::new();
  let external = config
    .gateway
    .http_token_env
    .as_deref()
    .map(|name| {
      config
        .gateway_credentials
        .get(name)
        .context("gateway HTTP token reference unavailable")
    })
    .transpose()?;
  if let Some(external) = external {
    ensure!(
      external.expose_secret() != internal_token.expose_secret(),
      "gateway HTTP token must differ from internal token"
    );
  }
  for token in std::iter::once(&internal_token).chain(external) {
    storage
      .bootstrap_access_token(token.expose_secret())
      .await
      .context("initialize gateway access")?;
    let principal = storage
      .authenticate(token.expose_secret())
      .await
      .map_err(|_| anyhow::anyhow!("gateway access unavailable"))?
      .context("gateway credential unavailable")?;
    allowed_ids.insert(principal.key_id);
  }
  let access = Arc::new(CurrentAccess {
    storage: storage.clone(),
    allowed_ids,
  });
  let initial_key = if config.gateway.models.is_empty() {
    gateway_config.models.first().map(|model| {
      cache_key(
        &model.id,
        &model.api_base,
        model.api_key.as_ref().map_or("", Secret::expose_secret),
        model.context_tokens.unwrap_or(0) as u32,
      )
    })
  } else {
    None
  };
  let gateway = if gateway_config.models.is_empty() {
    None
  } else {
    Some(Gateway::new_with_access_and_privacy(
      gateway_config,
      decision.clone(),
      Arc::new(storage.clone()),
      access.clone(),
      privacy.clone(),
    )?)
  };
  let mut cache = HashMap::new();
  if let (Some(key), Some(gateway)) = (initial_key, gateway.as_ref()) {
    cache.insert(key, gateway.clone());
  }
  Ok(GatewayHost {
    gateway,
    internal_token,
    explicit: !config.gateway.models.is_empty(),
    decision,
    storage,
    access,
    cache: Mutex::new(cache),
    privacy,
  })
}

fn credential(config: &Config, name: Option<&str>) -> Result<Option<Secret>> {
  name
    .map(|name| {
      config
        .gateway_credentials
        .get(name)
        .map(|value| Secret::new(value.expose_secret().to_owned()))
        .context("gateway upstream credential reference unavailable")
    })
    .transpose()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn access_rejects_old_tokens_after_rotation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let storage = GatewayStorage::open(directory.path().join("gateway.sqlite")).await?;
    let old = "old-token-0000000000000000000000000000";
    let current = "current-token-000000000000000000000000";
    storage.bootstrap_access_token(old).await?;
    storage.bootstrap_access_token(current).await?;
    let principal = storage
      .authenticate(current)
      .await
      .map_err(|_| anyhow::anyhow!("access failed"))?
      .context("current token missing")?;
    let access = CurrentAccess {
      storage,
      allowed_ids: [principal.key_id].into_iter().collect(),
    };
    assert!(
      access
        .authorize(current)
        .await
        .map_err(|_| anyhow::anyhow!("access failed"))?
    );
    assert!(
      !access
        .authorize(old)
        .await
        .map_err(|_| anyhow::anyhow!("access failed"))?
    );
    assert!(
      !access
        .authorize("invalid")
        .await
        .map_err(|_| anyhow::anyhow!("access failed"))?
    );
    Ok(())
  }
}
