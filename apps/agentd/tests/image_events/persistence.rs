use agent_runtime::{Result, RuntimeError, SessionMutation, SessionPersistence};
use agent_storage::Store;

/// Test-owned adapter: the storage crate does not depend on Harness/runtime.
pub struct Persistence(pub Store);
#[async_trait::async_trait]
impl SessionPersistence for Persistence {
  async fn load(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
    self.0.load_snapshot(session_id).await.map_err(failure)
  }
  async fn commit(
    &self,
    session_id: &str,
    owner_epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> Result<()> {
    self
      .0
      .commit_snapshot_with_events(
        session_id,
        owner_epoch,
        &snapshot,
        &mutation.kind,
        mutation.payload,
        &mutation.events,
      )
      .await
      .map_err(failure)
  }
}
fn failure(error: anyhow::Error) -> RuntimeError {
  RuntimeError::Persistence(agent_storage::public_error_message(&error))
}
