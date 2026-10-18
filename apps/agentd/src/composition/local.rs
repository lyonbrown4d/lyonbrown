use crate::config::Config;
use agent_security::{Identity, NodeLease};
use fluxdi::{Error, Shared, static_module};
use std::sync::Arc;
pub struct LocalNode {
  pub node_id: String,
  pub token: Arc<String>,
  pub lease: Arc<NodeLease>,
}
static_module! {
  pub(super) struct SecurityModule {
    #[scope(root)]
    local: LocalNode = local(config);
    external config: Config;
  }
}
fn local(config: Shared<Config>) -> Result<Shared<LocalNode>, Error> {
  let open = || -> anyhow::Result<LocalNode> {
    let data = config.data_path()?;
    let lease = NodeLease::acquire(&data)?;
    let identity = Identity::load(&data)?;
    let token = agent_security::token(&data)?;
    Ok(LocalNode {
      node_id: identity.node_id,
      token: Arc::new(token),
      lease: Arc::new(lease),
    })
  };
  open()
    .map(Shared::new)
    .map_err(|error| Error::factory_failed("LocalNode", &error.to_string()))
}
