use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;

impl AgentClient {
  /// A filtered page, not a claim that the whole session has no blocking tasks.
  pub async fn recovery_tasks(&self, session: &str, after: &str) -> Result<v1::Tasks> {
    let mut page = self.list_tasks(session, after, 100).await?;
    page.tasks.retain(|task| {
      !matches!(
        task.status.as_str(),
        "completed" | "failed" | "cancelled" | "rejected"
      )
    });
    Ok(page)
  }

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
