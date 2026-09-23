use super::*;

impl Kernel {
  pub(super) fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
    let next = self
      .state
      .transition(&event)
      .map_err(|error| AgentError::Transition(error.to_string()))?;
    if let Some(writer) = &self.writer {
      writer
        .record(event.clone())
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    }
    self.state = next;
    self.bus.emit(event);
    Ok(())
  }

  pub(super) fn record_error(&mut self, message: String) -> Result<(), AgentError> {
    self.emit(AgentEvent::Error {
      session_id: self.session.id,
      message: self.broker.redact(&message),
      ts: Utc::now(),
    })
  }

  pub(super) fn cancel_turn(&mut self, turn_id: TurnId) -> Result<(), AgentError> {
    self.emit(AgentEvent::TurnCancelled {
      session_id: self.session.id,
      turn_id,
      ts: Utc::now(),
    })
  }

  pub(super) fn active_skills(&self, prompt: &str) -> Result<Vec<Skill>, String> {
    let mut active = BTreeMap::new();
    for name in self
      .agent_profile
      .default_skills
      .iter()
      .chain(self.workspace_profile.enabled_skills.iter())
    {
      let skill = self
        .skills
        .get(name)
        .ok_or_else(|| format!("configured skill `{name}` is not registered"))?;
      active.insert(skill.name.clone(), skill.clone());
    }
    for skill in self.skills.match_prompt(prompt) {
      active.insert(skill.name.clone(), skill.clone());
    }
    Ok(active.into_values().collect())
  }
}
