use agent_client::AgentClient;
use fluxdi::{Error, Shared, static_module};

use crate::config::ResolvedConfig;

// Reconnection creates a fresh authenticated channel; task submission is never replayed here.
static_module! {
  pub struct ClientModule {
    external config: ResolvedConfig;
    #[scope(transient)]
    async client: AgentClient = connect(config);
  }
}

async fn connect(config: Shared<ResolvedConfig>) -> Result<Shared<AgentClient>, Error> {
  connect_client(&config)
    .await
    .map(Shared::new)
    .map_err(|error| Error::factory_failed("AgentClient", &error.to_string()))
}

pub(crate) async fn connect_client(config: &ResolvedConfig) -> anyhow::Result<AgentClient> {
  if config.endpoint == agent_launch::model::LOCAL_ENDPOINT {
    let mut options = agent_launch::LaunchOptions::bundled(["sessions", "runs", "tasks.manage"])?;
    options.data_dir = config.data_dir.clone();
    agent_launch::ensure_daemon(options).await
  } else {
    let token = config.token.as_deref().ok_or_else(|| anyhow::anyhow!(
      "remote connections require LYONBROWN_TOKEN or --token; local auth.token is not sent to remote endpoints"))?;
    AgentClient::connect(&config.endpoint, token.trim()).await
  }
}
