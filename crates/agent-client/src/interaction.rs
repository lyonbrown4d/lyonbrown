use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn interaction_policy(&self, session_id: &str) -> Result<v1::InteractionPolicy> {
    Ok(
      self
        .inner
        .clone()
        .get_interaction_policy(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn set_interaction_mode(
    &self,
    session_id: &str,
    owner_epoch: u64,
    expected_revision: u64,
    mode: v1::InteractionMode,
  ) -> Result<v1::InteractionPolicy> {
    Ok(
      self
        .inner
        .clone()
        .set_interaction_mode(v1::SetInteractionModeRequest {
          session_id: session_id.into(),
          owner_epoch,
          expected_revision,
          mode: mode as i32,
        })
        .await?
        .into_inner(),
    )
  }
}

pub fn tool_availability_label(tool: &v1::ToolAvailability) -> &'static str {
  use v1::ToolAvailabilityState as S;
  match S::try_from(tool.state).ok() {
    Some(S::Configured) => "configured; checked again when a run starts",
    Some(S::Available) => "available in the current run",
    Some(S::ModeRestricted) => "disabled by session mode",
    Some(S::ExecutionPlanRestricted) => "outside the current execution plan",
    Some(S::RuntimeUnavailable) => "execution environment not connected",
    _ => "availability unknown",
  }
}
