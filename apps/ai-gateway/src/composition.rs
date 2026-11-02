//! Typed application composition; HTTP request scopes remain at the transport boundary.
use crate::config::AppConfig;
use decision_core::{DecisionEngine, Policy};
use fluxdi::{Error, Shared, static_module};
use gateway_core::Gateway;
use gateway_storage::GatewayStorage;
use sha2::{Digest, Sha256};

/// Resolved only after durable bootstrap and policy validation have succeeded.
pub(crate) struct GatewayApplication {
  pub storage: Shared<GatewayStorage>,
  pub decision: Shared<DecisionEngine>,
  pub gateway: Shared<Gateway>,
}
pub(crate) struct InitializedStorage {
  storage: Shared<GatewayStorage>,
}

static_module! {
  pub(crate) struct GatewayModule {
    #[scope(root)]
    async storage: GatewayStorage = storage(config);
    #[scope(root)]
    async initialized: InitializedStorage = initialize(config, storage);
    #[scope(root)]
    async decision: DecisionEngine = decision(initialized);
    #[scope(root)]
    async gateway: Gateway = gateway(config, initialized, decision);
    #[scope(root)]
    async resolve: GatewayApplication = application(initialized, decision, gateway);
    external config: AppConfig;
  }
}

async fn storage(config: Shared<AppConfig>) -> Result<Shared<GatewayStorage>, Error> {
  config
    .validate()
    .map_err(|_| failed("AppConfig", "gateway configuration validation failed"))?;
  GatewayStorage::open(config.data_dir.join("gateway.sqlite"))
    .await
    .map(Shared::new)
    .map_err(|_| failed("GatewayStorage", "gateway database initialization failed"))
}

async fn initialize(
  config: Shared<AppConfig>,
  storage: Shared<GatewayStorage>,
) -> Result<Shared<InitializedStorage>, Error> {
  storage
    .bootstrap_super_admin(
      &config.admin_username,
      config.admin_password.expose_secret(),
    )
    .await
    .map_err(|_| failed("InitializedStorage", "super admin initialization failed"))?;
  storage
    .bootstrap_access_token(config.gateway.token.expose_secret())
    .await
    .map_err(|_| failed("InitializedStorage", "API access initialization failed"))?;
  // Only public model metadata participates in the persisted snapshot and revision.
  let registry: Vec<_> = config
    .gateway
    .models
    .iter()
    .map(|model| {
      serde_json::json!({
        "id":model.id,"upstream_model":model.upstream_model,"api_base":model.api_base,
        "tier":model.tier,"local_inference":model.local_inference,"capabilities":model.capabilities,
        "context_tokens":model.context_tokens,
      })
    })
    .collect();
  let document = serde_json::to_vec(&registry)
    .map_err(|_| failed("InitializedStorage", "model metadata serialization failed"))?;
  let revision = hex::encode(Sha256::digest(document));
  storage
    .seed_models(&revision, &config.gateway.models)
    .await
    .map_err(|_| failed("InitializedStorage", "model registry initialization failed"))?;
  Ok(Shared::new(InitializedStorage { storage }))
}

async fn decision(
  initialized: Shared<InitializedStorage>,
) -> Result<Shared<DecisionEngine>, Error> {
  let storage = &initialized.storage;
  let policy = match storage
    .load_policy()
    .await
    .map_err(|_| failed("DecisionEngine", "stored policy loading failed"))?
  {
    Some(policy) => policy,
    None => {
      let policy = Policy::default();
      storage
        .save_policy(policy.clone(), None)
        .await
        .map_err(|_| failed("DecisionEngine", "default policy initialization failed"))?;
      policy
    }
  };
  DecisionEngine::new(policy, Vec::new())
    .map(Shared::new)
    .map_err(|_| failed("DecisionEngine", "stored policy validation failed"))
}

async fn gateway(
  config: Shared<AppConfig>,
  initialized: Shared<InitializedStorage>,
  decision: Shared<DecisionEngine>,
) -> Result<Shared<Gateway>, Error> {
  let storage = initialized.storage.clone();
  Gateway::new_with_access(config.gateway.clone(), decision, storage.clone(), storage)
    .map(Shared::new)
    .map_err(|_| failed("Gateway", "validated gateway assembly failed"))
}

async fn application(
  initialized: Shared<InitializedStorage>,
  decision: Shared<DecisionEngine>,
  gateway: Shared<Gateway>,
) -> Result<Shared<GatewayApplication>, Error> {
  Ok(Shared::new(GatewayApplication {
    storage: initialized.storage.clone(),
    decision,
    gateway,
  }))
}

fn failed(service: &'static str, message: &'static str) -> Error {
  Error::factory_failed(service, message)
}
