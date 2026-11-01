use crate::{AgentClient, v1};
impl AgentClient {
  pub async fn hub_operation(&self, request: v1::HubRequest) -> anyhow::Result<String> {
    Ok(
      self
        .models
        .clone()
        .hub_operation(request)
        .await?
        .into_inner()
        .json,
    )
  }
  /// Empty session returns the authenticated global catalog without a selection.
  /// Non-empty session must exist and includes its current model selection.
  pub async fn list_models(&self, session: &str) -> anyhow::Result<Vec<v1::ModelCandidate>> {
    Ok(
      self
        .models
        .clone()
        .list_models(v1::SessionRequest {
          session_id: session.into(),
        })
        .await?
        .into_inner()
        .models,
    )
  }
  pub async fn select_model(
    &self,
    session: &str,
    epoch: u64,
    model: &str,
    execution: &str,
  ) -> anyhow::Result<v1::ModelSelection> {
    Ok(
      self
        .models
        .clone()
        .select_model(v1::SelectModelRequest {
          session_id: session.into(),
          owner_epoch: epoch,
          model_id: model.into(),
          execution_id: execution.into(),
        })
        .await?
        .into_inner(),
    )
  }
}
