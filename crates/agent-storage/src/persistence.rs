use crate::Store;
use agent_runtime::{Result, RuntimeError, SessionMutation, SessionPersistence};
#[async_trait::async_trait]
impl SessionPersistence for Store {
  async fn load(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
    self
      .load_snapshot(session_id)
      .map_err(|e| RuntimeError::Persistence(e.to_string()))
  }
  async fn commit(
    &self,
    session_id: &str,
    owner_epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> Result<()> {
    self
      .commit_snapshot(
        session_id,
        owner_epoch,
        &snapshot,
        &mutation.kind,
        mutation.payload,
      )
      .map_err(|e| RuntimeError::Persistence(e.to_string()))
  }
}
