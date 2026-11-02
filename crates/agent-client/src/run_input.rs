use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  /// Submit once. A lost response must be inspected using the same request ID.
  pub async fn submit_run_input(&self, input: v1::RunInputRequest) -> Result<v1::RunInputReceipt> {
    Ok(
      self
        .inner
        .clone()
        .submit_run_input(input)
        .await?
        .into_inner(),
    )
  }
  pub async fn cancel_run_input(
    &self,
    run: &str,
    epoch: u64,
    entry: &str,
  ) -> Result<v1::RunInputReceipt> {
    Ok(
      self
        .inner
        .clone()
        .cancel_run_input(v1::CancelRunInputRequest {
          run_id: run.into(),
          owner_epoch: epoch,
          entry_id: entry.into(),
        })
        .await?
        .into_inner(),
    )
  }
  /// Bounded Session snapshot: all pending plus up to 100 completed inputs.
  /// Large prompts are explicit previews, never reconstructed submission text.
  pub async fn run_input_snapshot(&self, run: &str) -> Result<v1::RunInputs> {
    Ok(
      self
        .inner
        .clone()
        .list_run_inputs(v1::RunRequest { run_id: run.into() })
        .await?
        .into_inner(),
    )
  }
  pub async fn list_run_inputs(&self, run: &str) -> Result<Vec<v1::RunInputReceipt>> {
    Ok(self.run_input_snapshot(run).await?.inputs)
  }
}
