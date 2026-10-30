use std::path::PathBuf;

use agent_client::local_state::LocalStateStore;

use crate::local_state::ClientState;

/// Open and load client state without blocking the daemon action executor.
pub(crate) async fn load(
  root: PathBuf,
  endpoint: String,
  node_id: String,
) -> anyhow::Result<(LocalStateStore, Option<ClientState>)> {
  tokio::task::spawn_blocking(move || {
    let store = LocalStateStore::new(root, "tui", &endpoint, &node_id)?;
    let state = store.load::<ClientState>()?;
    Ok((store, state))
  })
  .await?
}

/// Save one debounced snapshot off the async action executor and return its locked store.
pub(crate) async fn save(
  store: LocalStateStore,
  state: ClientState,
) -> anyhow::Result<(LocalStateStore, anyhow::Result<()>)> {
  Ok(
    tokio::task::spawn_blocking(move || {
      let result = store.save(&state);
      (store, result)
    })
    .await?,
  )
}
