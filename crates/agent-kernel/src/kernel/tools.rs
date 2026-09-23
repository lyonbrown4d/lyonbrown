use super::*;

impl Kernel {
  pub(super) async fn run_tools(
    &mut self,
    turn_id: TurnId,
    step_id: StepId,
    text: String,
    tool_calls: Vec<agent_core::ToolInvocation>,
  ) -> Result<(), AgentError> {
    let assistant_message = ChatMessage {
      role: Role::Assistant,
      content: text.clone(),
      tool_call_id: None,
      tool_name: None,
      tool_calls: tool_calls.clone(),
    };
    self
      .session
      .push_unbudgeted_message(assistant_message.clone());

    let call_ids = tool_calls.iter().map(|call| call.id).collect();
    for invocation in tool_calls {
      self.emit(AgentEvent::ToolRequested {
        step_id,
        invocation: invocation.clone(),
        ts: Utc::now(),
      })?;
      let mut result = match self.tools.get(&invocation.name) {
        None => ToolResult {
          invocation_id: invocation.id,
          ok: false,
          output: String::new(),
          truncated: false,
          duration_ms: 0,
          error: Some(format!("tool not found: {}", invocation.name)),
        },
        Some(tool) => match self.approval.approve(tool.definition(), &invocation) {
          ApprovalDecision::Deny | ApprovalDecision::DenyAll => ToolResult {
            invocation_id: invocation.id,
            ok: false,
            output: String::new(),
            truncated: false,
            duration_ms: 0,
            error: Some("tool denied by approval policy".into()),
          },
          ApprovalDecision::Allow | ApprovalDecision::AllowSession => {
            self.emit(AgentEvent::ToolApproved {
              step_id,
              tool_call_id: invocation.id,
              by: "policy".into(),
              ts: Utc::now(),
            })?;
            self.emit(AgentEvent::ToolStarted {
              step_id,
              tool_call_id: invocation.id,
              ts: Utc::now(),
            })?;
            let executor = ToolExecutor::new(self.tools.clone(), self.tool_context.clone());
            match executor.execute(&invocation).await {
              Ok(result) => result,
              Err(ToolExecutionError::Cancelled) => {
                self.cancel_turn(turn_id)?;
                return Err(AgentError::Cancelled);
              }
              Err(error) => ToolResult {
                invocation_id: invocation.id,
                ok: false,
                output: String::new(),
                truncated: false,
                duration_ms: 0,
                error: Some(error.to_string()),
              },
            }
          }
        },
      };
      result.output = self.broker.redact(&result.output);
      result.error = result.error.map(|error| self.broker.redact(&error));
      let observation = if result.ok {
        result.output.clone()
      } else {
        serde_json::json!({
            "ok": false,
            "output": result.output,
            "error": result.error,
        })
        .to_string()
      };
      self.emit(AgentEvent::ToolCompleted {
        step_id,
        result: result.clone(),
        ts: Utc::now(),
      })?;
      self.emit(AgentEvent::ObservationCreated {
        step_id,
        observation_id: ObservationId::new(),
        ts: Utc::now(),
      })?;
      let tool_message = ChatMessage {
        role: Role::Tool,
        content: observation.clone(),
        tool_call_id: Some(invocation.id),
        tool_name: Some(invocation.name.clone()),
        tool_calls: vec![],
      };
      self
        .session
        .push_tool_observation(tool_message, &assistant_message);
    }

    self.emit(AgentEvent::StepCompleted {
      step_id,
      outcome: StepOutcome {
        stop_reason: StopReason::ToolUse,
        text,
        tool_calls: call_ids,
        escalated: false,
      },
      ts: Utc::now(),
    })?;
    Ok(())
  }
}
