use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;

impl AgentClient {
  /// Submit once using the observed session fence; no retry or epoch refresh.
  pub async fn start_run_owned(
    &self,
    session_id: &str,
    owner_epoch: u64,
    prompt: &str,
    idempotency_key: &str,
  ) -> Result<v1::Run> {
    Ok(
      self
        .inner
        .clone()
        .start_run(v1::StartRunRequest {
          session_id: session_id.into(),
          owner_epoch,
          prompt: prompt.into(),
          idempotency_key: idempotency_key.into(),
        })
        .await?
        .into_inner(),
    )
  }
}
