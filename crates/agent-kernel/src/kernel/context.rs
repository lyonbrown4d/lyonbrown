use super::*;

impl Kernel {
  pub(super) fn build_model_request(
    &mut self,
    user_text: &str,
    step_id: StepId,
    selected: &SchedulerSelection,
    step_context: &StepContext,
    reason: &str,
  ) -> Result<ModelRequest, AgentError> {
    let tool_definitions = self.tools.definitions();
    let max_tokens = self
      .model_options
      .max_tokens
      .unwrap_or(selected.capabilities.max_output_tokens)
      .min(selected.capabilities.max_output_tokens);
    let input_budget = u64::from(
      selected
        .capabilities
        .context_window
        .saturating_sub(max_tokens),
    );
    let request_tools = if step_context.needs_tools && selected.capabilities.supports_tools {
      tool_definitions
    } else {
      Vec::new()
    };
    let estimator = TokenEstimator::default();
    let fixed_context_tokens = serialized_context_tokens(&estimator, &[], &request_tools)?;
    let token_budget = match input_budget.checked_sub(fixed_context_tokens) {
      Some(budget) => budget,
      None => {
        let error = AgentError::Context(format!(
          "tool definitions require {fixed_context_tokens} tokens but input budget is {input_budget}"
        ));
        self.record_error(error.to_string())?;
        return Err(error);
      }
    };
    let active_skills = match self.active_skills(user_text) {
      Ok(skills) => skills,
      Err(error) => {
        let error = AgentError::Context(error);
        self.record_error(error.to_string())?;
        return Err(error);
      }
    };
    let budget_items = self.session.budget_items();
    let compiled = match self.prompt_compiler.compile(CompileRequest {
      profile: &self.agent_profile,
      model: &selected.capabilities,
      workspace: &self.workspace_profile,
      skills: &active_skills,
      tool_guidance: None,
      context_items: &budget_items,
      token_budget,
    }) {
      Ok(compiled) => compiled,
      Err(error) => {
        let error = AgentError::Context(error.to_string());
        self.record_error(error.to_string())?;
        return Err(error);
      }
    };
    let request_messages = self.session.selected_messages(&compiled.context_items);
    let system_tokens = serialized_system_tokens(&estimator, &compiled.system_prompt)?;
    let context_tokens = serialized_context_tokens(&estimator, &request_messages, &request_tools)?;
    let finalized_tokens = system_tokens
      .checked_add(context_tokens)
      .ok_or_else(|| AgentError::Context("finalized request token count overflowed u64".into()))?;
    if finalized_tokens > input_budget {
      let error = AgentError::Context(format!(
        "finalized request requires {finalized_tokens} tokens but input budget is {input_budget}"
      ));
      self.record_error(error.to_string())?;
      return Err(error);
    }
    let system_tokens = u32::try_from(system_tokens)
      .map_err(|_| AgentError::Context("compiled system token count exceeds u32".into()))?;
    let context_tokens = u32::try_from(context_tokens)
      .map_err(|_| AgentError::Context("compiled context token count exceeds u32".into()))?;
    let dropped = u32::try_from(compiled.dropped)
      .map_err(|_| AgentError::Context("dropped context count exceeds u32".into()))?;
    self.emit(AgentEvent::ContextPrepared {
      step_id,
      system_tokens,
      context_tokens,
      dropped,
      ts: Utc::now(),
    })?;
    self.emit(AgentEvent::ModelSelected {
      step_id,
      provider: selected.decision.provider.clone(),
      model: selected.capabilities.model.clone(),
      reason: reason.to_owned(),
      ts: Utc::now(),
    })?;

    let request = ModelRequest {
      model: selected.capabilities.model.clone(),
      system: Some(compiled.system_prompt),
      messages: request_messages,
      tools: request_tools,
      max_tokens,
      temperature: self.model_options.temperature,
      json_mode: false,
      thinking: if selected.capabilities.supports_thinking {
        self.model_options.thinking
      } else {
        Some(false)
      },
      stop: vec![],
    };
    self.last_requests.push(request.clone());
    self.emit(AgentEvent::ModelStarted {
      step_id,
      ts: Utc::now(),
    })?;
    Ok(request)
  }
}
