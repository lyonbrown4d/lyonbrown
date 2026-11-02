use super::local::LocalNode;
use crate::config::Config;
use agent_storage::Store;
use fluxdi::{Error, Shared, static_module};
static_module! {
  pub(super) struct StorageModule {
    #[scope(root)]
    async store: Store = store(config,local);
    external config: Config;
    external local: LocalNode;
  }
}
async fn store(config: Shared<Config>, _local: Shared<LocalNode>) -> Result<Shared<Store>, Error> {
  let result = if let Some(url) = &config.database_url {
    Store::connect(url).await
  } else {
    match config.data_path() {
      Ok(path) => Store::open(path.join("agent.sqlite3")).await,
      Err(error) => Err(error),
    }
  };
  result
    .map(Shared::new)
    .map_err(|_| Error::factory_failed("Store", "database connection or migration failed"))
}
