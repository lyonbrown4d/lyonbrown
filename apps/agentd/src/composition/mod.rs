//! Typed composition root. Business services receive concrete ports, never an Injector.
mod local;
mod persistence;
use crate::{Daemon, config::Config, state::RuntimeServices};
use fluxdi::{Error, Shared, static_module};
use local::{LocalNode, SecurityModule};
use persistence::StorageModule;

static_module! {
  struct DaemonModule {
    import persistence: StorageModule { store: agent_storage::Store; };
    #[scope(root)]
    services: RuntimeServices = runtime_services(config);
    #[scope(root)]
    mcp: crate::mcp::Connections = mcp_connections();
    #[scope(root)]
    decision: agent_decision::DecisionEngine = decision(config,models);
    #[scope(root)]
    daemon: Daemon = daemon(config,local,store,services,decision,models,mcp);
    external config: Config;
    external local: LocalNode;
    external models: crate::models::Models;
  }
}
pub fn build(mut config: Config, models: crate::models::Models) -> anyhow::Result<Daemon> {
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
  Ok((*application.daemon()?).clone())
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
fn daemon(
  config: Shared<Config>,
  local: Shared<LocalNode>,
  store: Shared<agent_storage::Store>,
  services: Shared<RuntimeServices>,
  decision: Shared<agent_decision::DecisionEngine>,
  models: Shared<crate::models::Models>,
  mcp: Shared<crate::mcp::Connections>,
) -> Result<Shared<Daemon>, Error> {
  let secret_root = config
    .data_path()
    .map_err(|error| Error::factory_failed("SecretStore", &error.to_string()))?
    .join("provider-secrets");
  let provider_secrets = agent_secrets::SecretStore::open(secret_root)
    .map_err(|error| Error::factory_failed("SecretStore", &error.to_string()))?;
  Ok(Shared::new(Daemon {
    config: (*config).clone(),
    store: (*store).clone(),
    node_id: local.node_id.clone(),
    token: local.token.clone(),
    services,
    decision,
    models,
    mcp,
    provider_secrets: std::sync::Arc::new(provider_secrets),
    _lease: local.lease.clone(),
  }))
}
