use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn list_plans(&self, input: v1::ListWorkflowPlansRequest) -> Result<v1::WorkflowPlans> {
    Ok(self.workflow.clone().list_plans(input).await?.into_inner())
  }
  pub async fn get_plan(&self, session_id: &str, plan_id: &str) -> Result<v1::WorkflowPlan> {
    Ok(
      self
        .workflow
        .clone()
        .get_plan(v1::GetWorkflowPlanRequest {
          session_id: session_id.into(),
          plan_id: plan_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn mutate_plan(
    &self,
    input: v1::MutateWorkflowPlanRequest,
  ) -> Result<v1::WorkflowPlan> {
    Ok(self.workflow.clone().mutate_plan(input).await?.into_inner())
  }
  pub async fn list_changes(
    &self,
    input: v1::ListWorkflowChangesRequest,
  ) -> Result<v1::WorkflowChanges> {
    Ok(
      self
        .workflow
        .clone()
        .list_changes(input)
        .await?
        .into_inner(),
    )
  }
  pub async fn get_change(&self, session_id: &str, change_id: &str) -> Result<v1::WorkflowChange> {
    Ok(
      self
        .workflow
        .clone()
        .get_change(v1::GetWorkflowChangeRequest {
          session_id: session_id.into(),
          change_id: change_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn preview_undo(
    &self,
    input: v1::PreviewWorkflowUndoRequest,
  ) -> Result<v1::WorkflowChange> {
    Ok(
      self
        .workflow
        .clone()
        .preview_undo(input)
        .await?
        .into_inner(),
    )
  }
  pub async fn apply_change(
    &self,
    input: v1::ApplyWorkflowChangeRequest,
  ) -> Result<v1::WorkflowChange> {
    Ok(
      self
        .workflow
        .clone()
        .apply_change(input)
        .await?
        .into_inner(),
    )
  }
}
