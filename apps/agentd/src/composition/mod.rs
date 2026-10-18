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
pub fn build(config: Config, models: crate::models::Models) -> anyhow::Result<Daemon> {
  crate::commands::validate(&config)?;
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
  Ok(Shared::new(Daemon {
    config: (*config).clone(),
    store: (*store).clone(),
    node_id: local.node_id.clone(),
    token: local.token.clone(),
    services,
    decision,
    models,
    mcp,
    _lease: local.lease.clone(),
  }))
}
