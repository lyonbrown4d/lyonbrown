use super::*;

impl Kernel {
  pub(super) async fn prepare_step(
    &mut self,
    user_text: &str,
    turn_id: TurnId,
    step_id: StepId,
    steps: usize,
    escalations: usize,
    used_tool: bool,
  ) -> Result<PreparedStep, AgentError> {
    let tool_definitions = self.tools.definitions();
    let mut step_context = StepContext::new(user_text);
    step_context.needs_tools =
      !tool_definitions.is_empty() && (used_tool || step_needs_tools(user_text));
    step_context.previous_steps = steps - 1;
    step_context.escalated = escalations > 0;
    step_context.provider = self.model_options.provider.clone();
    step_context.model = self.model_options.model.clone();
    step_context.classification = if self
      .session
      .working_memory
      .iter()
      .any(|item| item.classification == agent_core::DataClassification::Secret)
    {
      agent_core::DataClassification::Secret
    } else if self.session.working_memory.iter().any(|item| {
      item.classification == agent_core::DataClassification::Confidential
        || item.kind == ContextItemKind::ToolObservation
    }) {
      agent_core::DataClassification::Confidential
    } else {
      agent_core::DataClassification::Public
    };
    let cancellation = self.cancellation.clone();
    let selected = match tokio::select! {
        () = cancellation.cancelled() => {
            self.cancel_turn(turn_id)?;
            return Err(AgentError::Cancelled);
        }
        selected = self.scheduler.decide(&step_context) => selected,
    } {
      Ok(selected) => selected,
      Err(error) => {
        let error = AgentError::Model(self.broker.redact(&error.to_string()));
        self.record_error(error.to_string())?;
        return Err(error);
      }
    };
    let request = self.build_model_request(
      user_text,
      step_id,
      &selected,
      &step_context,
      &selected.decision.reason,
    )?;
    Ok(PreparedStep {
      selected,
      request,
      step_context,
    })
  }
}
