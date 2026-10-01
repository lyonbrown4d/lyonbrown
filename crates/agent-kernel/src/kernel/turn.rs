use super::*;

impl Kernel {
  pub(super) async fn run_turn_inner(
    &mut self,
    user_text: &str,
  ) -> Result<Vec<AgentEvent>, AgentError> {
    let log_start = self.bus.log().len();
    if !matches!(
      self.state,
      KernelState::Idle | KernelState::Failed { .. } | KernelState::Cancelled
    ) {
      return Err(AgentError::Transition(format!(
        "cannot start turn from {:?}",
        self.state
      )));
    }

    let now = Utc::now();
    if !self.session.started {
      self.emit(AgentEvent::SessionStarted {
        session_id: self.session.id,
        ts: now,
      })?;
      self.session.started = true;
    }

    let turn_id = TurnId::new();
    self.emit(AgentEvent::UserMessageReceived {
      session_id: self.session.id,
      turn_id,
      text: user_text.to_owned(),
      ts: Utc::now(),
    })?;
    self.emit(AgentEvent::TurnStarted {
      session_id: self.session.id,
      turn_id,
      ts: Utc::now(),
    })?;
    self.session.push_message(
      ChatMessage {
        role: Role::User,
        content: user_text.to_owned(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: vec![],
      },
      ContextItemKind::Message,
      "user",
    );

    let mut active_text = user_text.to_owned();
    let mut steps = 0;
    let mut escalations = 0;
    let mut used_tool = false;
    loop {
      if self.cancellation.is_cancelled() {
        self.cancel_turn(turn_id)?;
        return Err(AgentError::Cancelled);
      }
      if let Some(steering) = self.apply_pending_steering(turn_id)? {
        active_text = steering;
      }
      if steps >= self.limits.max_steps_per_turn {
        let message = format!(
          "max steps per turn exceeded ({})",
          self.limits.max_steps_per_turn
        );
        self.emit(AgentEvent::Error {
          session_id: self.session.id,
          message: message.clone(),
          ts: Utc::now(),
        })?;
        return Err(AgentError::LimitExceeded(message));
      }
      steps += 1;

      let step_id = StepId::new();
      self.emit(AgentEvent::StepStarted {
        session_id: self.session.id,
        turn_id,
        step_id,
        ts: Utc::now(),
      })?;
      let prepared = self
        .prepare_step(
          &active_text,
          turn_id,
          step_id,
          steps,
          escalations,
          used_tool,
        )
        .await?;
      let (selected, response) = self.start_model(turn_id, step_id, prepared).await?;
      let mut stream = response.events;
      let mut text = String::new();
      let mut tool_calls = Vec::new();
      let mut usage = TokenUsage::default();
      let mut stop_reason = None;
      let cancellation = self.cancellation.clone();
      loop {
        let event = tokio::select! {
            () = cancellation.cancelled() => {
                self.cancel_turn(turn_id)?;
                return Err(AgentError::Cancelled);
            }
            event = stream.next() => event,
        };
        let Some(event) = event else { break };
        let event = match event {
          Ok(event) => event,
          Err(error) => {
            let error = AgentError::Model(self.broker.redact(&error.to_string()));
            self.record_error(error.to_string())?;
            return Err(error);
          }
        };
        match event {
          ModelEvent::Delta(delta) => {
            text.push_str(&delta);
            self.emit(AgentEvent::ModelDelta { step_id, delta })?;
          }
          ModelEvent::ToolCall(invocation) => tool_calls.push(invocation),
          ModelEvent::Usage(value) => usage = value,
          ModelEvent::Done {
            stop_reason: reason,
          } => stop_reason = Some(reason),
        }
      }
      let stop_reason = match stop_reason {
        Some(stop_reason) => stop_reason,
        None => {
          let error = AgentError::Model("provider stream ended without a Done event".into());
          self.record_error(error.to_string())?;
          return Err(error);
        }
      };
      self.emit(AgentEvent::ModelCompleted {
        step_id,
        usage,
        ts: Utc::now(),
      })?;

      if let Some(steering) = self.apply_pending_steering(turn_id)? {
        active_text = steering;
        continue;
      }

      if tool_calls.is_empty() && matches!(stop_reason, StopReason::ToolUse) {
        let error = AgentError::Model("provider returned ToolUse without any tool calls".into());
        self.record_error(error.to_string())?;
        return Err(error);
      }
      if !tool_calls.is_empty() {
        self.run_tools(turn_id, step_id, text, tool_calls).await?;
        used_tool = true;
        continue;
      }

      let mut outcome = StepOutcome {
        stop_reason,
        text: text.clone(),
        tool_calls: vec![],
        escalated: false,
      };
      match self.gate.evaluate(&outcome, &selected.capabilities) {
        GateVerdict::Pass => {
          self.session.push_message(
            ChatMessage {
              role: Role::Assistant,
              content: text.clone(),
              tool_call_id: None,
              tool_name: None,
              tool_calls: vec![],
            },
            ContextItemKind::Message,
            "assistant",
          );
          self.emit(AgentEvent::StepCompleted {
            step_id,
            outcome,
            ts: Utc::now(),
          })?;
          self.emit(AgentEvent::TurnCompleted {
            turn_id,
            answer: text,
            ts: Utc::now(),
          })?;
          return Ok(self.bus.log()[log_start..].to_vec());
        }
        GateVerdict::Escalate(_reason) if escalations < self.limits.max_escalations => {
          escalations += 1;
          outcome.escalated = true;
          self.emit(AgentEvent::StepCompleted {
            step_id,
            outcome,
            ts: Utc::now(),
          })?;
        }
        GateVerdict::Escalate(reason) => {
          self.emit(AgentEvent::Error {
            session_id: self.session.id,
            message: reason.clone(),
            ts: Utc::now(),
          })?;
          return Err(AgentError::LimitExceeded(reason));
        }
      }
    }
  }

  fn apply_pending_steering(&mut self, turn_id: TurnId) -> Result<Option<String>, AgentError> {
    let mut latest = None;
    while let Ok((generation, text)) = self.steering_rx.try_recv() {
      if generation != self.active_steering_generation {
        continue;
      }
      self.emit(AgentEvent::TurnSteered {
        session_id: self.session.id,
        turn_id,
        text: text.clone(),
        ts: Utc::now(),
      })?;
      self.session.push_message(
        ChatMessage {
          role: Role::User,
          content: text.clone(),
          tool_call_id: None,
          tool_name: None,
          tool_calls: vec![],
        },
        ContextItemKind::Message,
        "steer",
      );
      latest = Some(text);
    }
    Ok(latest)
  }
}
