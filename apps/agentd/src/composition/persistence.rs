use super::local::LocalNode;
use crate::config::Config;
use agent_storage::Store;
use fluxdi::{Error, Shared, static_module};
static_module! {
  pub(super) struct StorageModule {
    #[scope(root)]
    store: Store = store(config,local);
    external config: Config;
    external local: LocalNode;
  }
}
fn store(config: Shared<Config>, _local: Shared<LocalNode>) -> Result<Shared<Store>, Error> {
  let open = || -> anyhow::Result<Store> { Store::open(config.data_path()?.join("agent.sqlite3")) };
  open()
    .map(Shared::new)
    .map_err(|error| Error::factory_failed("Store", &error.to_string()))
}
