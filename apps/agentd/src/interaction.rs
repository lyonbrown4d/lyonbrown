use crate::{Daemon, service::db_error};
use agent_core::interaction::{InteractionMode, InteractionPolicy};
use agent_protocol::v1;
use tonic::Status;
impl Daemon {
  pub(crate) async fn interaction_status(
    &self,
    session_id: &str,
  ) -> Result<v1::InteractionPolicy, Status> {
    let session = self
      .store
      .session(session_id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::failed_precondition(
        "session belongs to another owner",
      ));
    }
    let policy = self
      .store
      .interaction_policy(session_id)
      .await
      .map_err(db_error)?;
    self.interaction_view(session_id, policy).await
  }
  async fn interaction_view(
    &self,
    session: &str,
    policy: InteractionPolicy,
  ) -> Result<v1::InteractionPolicy, Status> {
    let run = self
      .store
      .runs(session)
      .await
      .map_err(db_error)?
      .into_iter()
      .find(|run| {
        matches!(
          run.status.as_str(),
          "accepted" | "running" | "awaiting_approval" | "cancel_requested"
        )
      });
    let effective = if let Some(run) = &run {
      self
        .store
        .events(&run.id, 0, 1000)
        .await
        .map_err(db_error)?
        .into_iter()
        .find(|event| event.kind == "execution_plan")
        .and_then(|event| {
          serde_json::from_str::<agent_decision::ExecutionPlan>(&event.payload_json).ok()
        })
        .map(|plan| plan.allowed_tools)
    } else {
      None
    };
    let browser = self.browser.has_runtime().await;
    let computer = self.computer_runtime_info(session).await?.is_some();
    let mut tools = crate::decision::workspace_rule(
      !self.config.mcp.is_empty(),
      self.config.browser_tools,
      crate::commands::enabled(&self.config),
      !self.config.delegation_targets.is_empty(),
      self.config.computer_tools,
    )
    .tools
    .into_iter()
    .map(|tool| {
      use agent_decision::Capability;
      use v1::ToolAvailabilityState as S;
      let state = if !policy.mode.permits(tool.name()) {
        S::ModeRestricted
      } else if matches!(tool.capability(), Capability::Browser) && !browser
        || matches!(tool.capability(), Capability::Computer) && !computer
      {
        S::RuntimeUnavailable
      } else if run.is_some()
        && !effective
          .as_ref()
          .is_some_and(|tools| tools.contains(&tool))
      {
        S::ExecutionPlanRestricted
      } else if run.is_some() {
        S::Available
      } else {
        S::Configured
      };
      v1::ToolAvailability {
        name: tool.name().into(),
        available: matches!(state, S::Available | S::Configured),
        requires_approval: tool.requires_approval(),
        state: state as i32,
      }
    })
    .collect::<Vec<_>>();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v1::InteractionPolicy {
      revision: policy.revision,
      mode: match policy.mode {
        InteractionMode::ReadOnly => v1::InteractionMode::ReadOnly,
        InteractionMode::Plan => v1::InteractionMode::Plan,
        InteractionMode::Execute => v1::InteractionMode::Execute,
      } as i32,
      tools,
      run_id: run.map_or_else(String::new, |run| run.id),
    })
  }
  pub(crate) async fn change_interaction_mode(
    &self,
    input: v1::SetInteractionModeRequest,
  ) -> Result<v1::InteractionPolicy, Status> {
    let _submission = self.submission.lock().await;
    self.interaction_status(&input.session_id).await?;
    let mode = match v1::InteractionMode::try_from(input.mode).ok() {
      Some(v1::InteractionMode::ReadOnly) => InteractionMode::ReadOnly,
      Some(v1::InteractionMode::Plan) => InteractionMode::Plan,
      Some(v1::InteractionMode::Execute) => InteractionMode::Execute,
      _ => {
        return Err(Status::invalid_argument(
          "explicit interaction mode required",
        ));
      }
    };
    let policy = self
      .store
      .set_interaction_mode(
        &input.session_id,
        input.owner_epoch,
        input.expected_revision,
        mode,
      )
      .await
      .map_err(db_error)?;
    self.events.notify_waiters();
    self.interaction_view(&input.session_id, policy).await
  }
}
