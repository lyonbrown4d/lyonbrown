use std::time::Duration;

use agent_control::ControlPlane;
use agent_distributed::peers::PeerManager;
use agent_storage::Store;
use fluxdi::{Error, Shared, static_module};

use crate::config::Settings;

static_module! {
  pub struct ControlModule {
    external peers: PeerManager;
    external store: Store;
    external settings: Settings;
    #[scope(root)]
    control: ControlPlane = control(peers, store, settings);
  }
}
fn control(
  peers: Shared<PeerManager>,
  store: Shared<Store>,
  settings: Shared<Settings>,
) -> Result<Shared<ControlPlane>, Error> {
  ControlPlane::new(
    (*peers).clone(),
    (*store).clone(),
    Duration::from_secs(settings.lease_seconds),
    settings.queue_capacity,
  )
  .map(Shared::new)
  .map_err(|error| Error::factory_failed("ControlPlane", &error.to_string()))
}
