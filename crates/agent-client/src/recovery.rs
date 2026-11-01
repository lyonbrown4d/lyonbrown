use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;

impl AgentClient {
  pub async fn run_recovery(&self, run_id: &str) -> Result<v1::RunRecovery> {
    Ok(
      self
        .inner
        .clone()
        .get_run_recovery(v1::RunRequest {
          run_id: run_id.into(),
        })
        .await?
        .into_inner(),
    )
  }

  /// Execute once. Preserve the request identity and inspect after a lost response.
  pub async fn recover_run(&self, request: v1::RecoverRunRequest) -> Result<v1::RunRecoveryResult> {
    Ok(self.inner.clone().recover_run(request).await?.into_inner())
  }
}
