//! Typed composition root. Business services receive concrete ports, never an Injector.
mod local;
mod memory;
mod persistence;
mod private_embedding;
use crate::{Daemon, config::Config, state::RuntimeServices};
use fluxdi::{Error, Shared, static_module};
use local::{LocalNode, SecurityModule};
use persistence::StorageModule;

static_module! {
  struct DaemonModule {
    import persistence: StorageModule { async store: agent_storage::Store; };
    #[scope(root)]
    services: RuntimeServices = runtime_services(config);
    #[scope(root)]
    mcp: crate::mcp::Connections = mcp_connections();
    #[scope(root)]
    decision: agent_decision::DecisionEngine = decision(config,models);
    #[scope(root)]
    async memory: agent_memory::MemoryService = memory::service(config,store,provider_secrets,local);
    #[scope(root)]
    privacy: tokio::sync::Mutex<agent_privacy::PrivacyService> = privacy(config,local);
    #[scope(root)]
    async provider_secrets: agent_secrets::SecretStore = provider_secrets(config);
    #[scope(root)]
    async gateway: crate::gateway::GatewayHost = gateway(config,local,decision,provider_secrets,privacy);
    #[scope(root)]
    daemon: Daemon = daemon(config,local,store,services,decision,models,mcp,memory,gateway,provider_secrets,privacy);
    external config: Config;
    external local: LocalNode;
    external models: crate::models::Models;
  }
}
pub async fn build(mut config: Config, models: crate::models::Models) -> anyhow::Result<Daemon> {
  if config.provider_inventory.is_empty() {
    let selected = agent_config::provider_definition(&config.provider);
    config.provider_inventory = agent_config::PROVIDER_CATALOG
      .iter()
      .map(|definition| {
        let active = selected.is_some_and(|value| value.id == definition.id);
        agent_config::ProviderStatus {
          definition: *definition,
          selected: active,
          credential: agent_config::ProviderCredentialStatus::default(),
          configured_models: if active {
            vec![agent_config::ConfiguredProviderModel {
              id: config.model.clone(),
              selected: true,
            }]
          } else {
            Vec::new()
          },
        }
      })
      .collect();
  }
  crate::commands::validate(&config)?;
  anyhow::ensure!(
    (10..=300).contains(&config.computer_lease_secs),
    "computer lease must be within 10..=300 seconds"
  );
  agent_config::validate_delegation_targets(&config.delegation_targets)?;
  if let Some(relay) = &config.relay {
    relay.validate()?;
    anyhow::ensure!(
      config.peer.is_some(),
      "relay requires the peer runtime to be configured"
    );
  }
  anyhow::ensure!(
    (1..=128).contains(&config.max_runs),
    "max_runs must be between 1 and 128"
  );
  anyhow::ensure!(config.run_timeout_secs > 0, "run timeout must be positive");
  let config = Shared::new(config);
  let security = SecurityModule::new(config.clone());
  let local = security.local()?;
  let persistence = StorageModule::new(config.clone(), local.clone());
  let application = DaemonModule::new(persistence, config, local, Shared::new(models));
  Ok((*application.daemon().await?).clone())
}
fn runtime_services(config: Shared<Config>) -> Result<Shared<RuntimeServices>, Error> {
  Ok(Shared::new(RuntimeServices::new(config.max_runs)))
}
fn mcp_connections() -> Result<Shared<crate::mcp::Connections>, Error> {
  Ok(Shared::new(crate::mcp::Connections::default()))
}
fn decision(
  config: Shared<Config>,
  models: Shared<crate::models::Models>,
) -> Result<Shared<agent_decision::DecisionEngine>, Error> {
  crate::decision::build(&config, models)
    .map(Shared::new)
    .map_err(|error| Error::factory_failed("DecisionEngine", &error.to_string()))
}
#[allow(
  clippy::too_many_arguments,
  reason = "composition lists concrete application ports"
)]
fn daemon(
  config: Shared<Config>,
  local: Shared<LocalNode>,
  store: Shared<agent_storage::Store>,
  services: Shared<RuntimeServices>,
  decision: Shared<agent_decision::DecisionEngine>,
  models: Shared<crate::models::Models>,
  mcp: Shared<crate::mcp::Connections>,
  memory: Shared<agent_memory::MemoryService>,
  gateway: Shared<crate::gateway::GatewayHost>,
  provider_secrets: Shared<agent_secrets::SecretStore>,
  privacy: Shared<tokio::sync::Mutex<agent_privacy::PrivacyService>>,
) -> Result<Shared<Daemon>, Error> {
  Ok(Shared::new(Daemon {
    config: (*config).clone(),
    store: (*store).clone(),
    memory,
    gateway,
    privacy,
    node_id: local.node_id.clone(),
    token: local.token.clone(),
    services,
    decision,
    models,
    mcp,
    provider_secrets,
    _lease: local.lease.clone(),
  }))
}

async fn provider_secrets(
  config: Shared<Config>,
) -> Result<Shared<agent_secrets::SecretStore>, Error> {
  let root = config
    .data_path()
    .map_err(|_| Error::factory_failed("SecretStore", "data directory unavailable"))?
    .join("provider-secrets");
  tokio::task::spawn_blocking(move || agent_secrets::SecretStore::open(root))
    .await
    .map_err(|_| Error::factory_failed("SecretStore", "credential worker failed"))?
    .map(Shared::new)
    .map_err(|_| Error::factory_failed("SecretStore", "credential store unavailable"))
}
async fn gateway(
  config: Shared<Config>,
  local: Shared<LocalNode>,
  decision: Shared<agent_decision::DecisionEngine>,
  secrets: Shared<agent_secrets::SecretStore>,
  privacy: Shared<tokio::sync::Mutex<agent_privacy::PrivacyService>>,
) -> Result<Shared<crate::gateway::GatewayHost>, Error> {
  use agent_secrets::ExposeSecret;
  if config
    .gateway
    .http_token_env
    .as_ref()
    .and_then(|name| config.gateway_credentials.get(name))
    .is_some_and(|token| token.expose_secret() == local.token.as_str())
  {
    return Err(Error::factory_failed(
      "Gateway",
      "model API token must differ from daemon token",
    ));
  }
  crate::gateway::build(&config, decision.reusable(), secrets, privacy)
    .await
    .map(Shared::new)
    .map_err(|_| Error::factory_failed("Gateway", "gateway configuration or storage unavailable"))
}

fn privacy(
  config: Shared<Config>,
  local: Shared<LocalNode>,
) -> Result<Shared<tokio::sync::Mutex<agent_privacy::PrivacyService>>, Error> {
  let mut known = config.privacy_credentials();
  known.push(local.token.as_str().to_owned().into());
  agent_privacy::PrivacyService::new(&known)
    .map(tokio::sync::Mutex::new)
    .map(Shared::new)
    .map_err(|_| Error::factory_failed("PrivacyService", "privacy initialization failed"))
}
