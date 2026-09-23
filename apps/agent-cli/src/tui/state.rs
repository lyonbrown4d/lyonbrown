use agent_core::{AgentEvent, StepId};

use super::terminal_safe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
  User,
  Assistant,
  System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
  pub speaker: Speaker,
  pub text: String,
}

#[derive(Debug, Default)]
pub struct TuiState {
  transcript: Vec<TranscriptEntry>,
  status: String,
  model: String,
  reason: String,
  step: String,
  tool: String,
  usage: (u32, u32, u32),
  pulse: Vec<String>,
  streaming_step: Option<StepId>,
  pub(crate) input: String,
  pub(crate) busy: bool,
}

impl TuiState {
  pub fn transcript(&self) -> &[TranscriptEntry] {
    &self.transcript
  }

  pub fn status(&self) -> &str {
    if self.status.is_empty() {
      "idle"
    } else {
      &self.status
    }
  }

  pub fn reduce(&mut self, event: AgentEvent) {
    self.push_pulse(event.kind());
    match event {
      AgentEvent::UserMessageReceived { text, .. } => {
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::User,
          text: terminal_safe(&text),
        });
        self.status = "thinking".into();
        self.busy = true;
      }
      AgentEvent::TurnStarted { .. } => {
        self.status = "thinking".into();
        self.busy = true;
      }
      AgentEvent::StepStarted { step_id, .. } => {
        self.step = step_id.to_string();
        self.status = "preparing step".into();
      }
      AgentEvent::ContextPrepared { context_tokens, .. } => {
        self.status = format!("context ready · {context_tokens} tokens");
      }
      AgentEvent::ModelSelected {
        provider,
        model,
        reason,
        ..
      } => {
        self.model = format!("{}/{}", terminal_safe(&provider), terminal_safe(&model));
        self.reason = terminal_safe(&reason);
        self.status = "model selected".into();
      }
      AgentEvent::ModelFallback {
        to_provider,
        to_model,
        reason,
        ..
      } => {
        self.model = format!(
          "{}/{}",
          terminal_safe(&to_provider),
          terminal_safe(&to_model)
        );
        self.reason = terminal_safe(&reason);
        self.status = format!("model fallback · {}", self.model);
      }
      AgentEvent::ModelStarted { .. } => self.status = "streaming".into(),
      AgentEvent::ModelDelta { step_id, delta } => {
        let delta = terminal_safe(&delta);
        if self.streaming_step == Some(step_id) {
          if let Some(entry) = self.transcript.last_mut() {
            entry.text.push_str(&delta);
          }
        } else {
          self.transcript.push(TranscriptEntry {
            speaker: Speaker::Assistant,
            text: delta,
          });
          self.streaming_step = Some(step_id);
        }
        self.status = "streaming".into();
      }
      AgentEvent::ModelCompleted { usage, .. } => {
        self.usage.0 = self.usage.0.saturating_add(usage.input);
        self.usage.1 = self.usage.1.saturating_add(usage.output);
        self.usage.2 = self.usage.2.saturating_add(usage.cache_read);
        self.status = "model complete".into();
        self.streaming_step = None;
      }
      AgentEvent::ToolRequested { invocation, .. } => {
        self.tool = terminal_safe(&invocation.name);
        self.status = format!("tool requested · {}", self.tool);
      }
      AgentEvent::ToolApproved { .. } => self.status = "tool approved".into(),
      AgentEvent::ToolStarted { .. } => self.status = "tool running".into(),
      AgentEvent::ToolCompleted { result, .. } => {
        self.status = if result.ok {
          "tool complete".into()
        } else {
          format!(
            "tool error · {}",
            terminal_safe(&result.error.unwrap_or_else(|| "unknown".into()))
          )
        };
      }
      AgentEvent::StepCompleted { .. } => self.status = "step complete".into(),
      AgentEvent::TurnCompleted { answer, .. } => {
        let answer = terminal_safe(&answer);
        if !answer.is_empty()
          && !self
            .transcript
            .iter()
            .rev()
            .any(|entry| entry.speaker == Speaker::Assistant && entry.text == answer)
        {
          self.transcript.push(TranscriptEntry {
            speaker: Speaker::Assistant,
            text: answer,
          });
        }
        self.status = "idle".into();
        self.busy = false;
        self.streaming_step = None;
      }
      AgentEvent::TurnCancelled { .. } => {
        self.status = "cancelled".into();
        self.busy = false;
      }
      AgentEvent::Error { message, .. } => {
        self.status = "error".into();
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::System,
          text: terminal_safe(&message),
        });
        self.busy = false;
        self.streaming_step = None;
      }
      AgentEvent::SessionStarted { .. }
      | AgentEvent::ObservationCreated { .. }
      | AgentEvent::SessionClosed { .. } => {}
    }
  }

  pub(crate) fn mark_submitted(&mut self) {
    self.status = "starting turn".into();
    self.busy = true;
  }

  pub(crate) fn set_error(&mut self, message: impl Into<String>) {
    self.status = "error".into();
    self.transcript.push(TranscriptEntry {
      speaker: Speaker::System,
      text: terminal_safe(&message.into()),
    });
    self.busy = false;
  }

  fn push_pulse(&mut self, kind: &str) {
    self.pulse.push(kind.to_owned());
    if self.pulse.len() > 8 {
      self.pulse.remove(0);
    }
  }

  pub(crate) fn model(&self) -> &str {
    if self.model.is_empty() {
      "—"
    } else {
      &self.model
    }
  }
  pub(crate) fn reason(&self) -> &str {
    if self.reason.is_empty() {
      "—"
    } else {
      &self.reason
    }
  }
  pub(crate) fn usage(&self) -> (u32, u32, u32) {
    self.usage
  }
  pub(crate) fn step(&self) -> &str {
    if self.step.is_empty() {
      "—"
    } else {
      &self.step
    }
  }
  pub(crate) fn tool(&self) -> &str {
    if self.tool.is_empty() {
      "—"
    } else {
      &self.tool
    }
  }
  pub(crate) fn pulse(&self) -> &[String] {
    &self.pulse
  }
}
