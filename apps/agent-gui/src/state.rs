use agent_app::{AgentCommand, ModelSetupView, RuntimePhase, RuntimeSnapshot};

use crate::{
  copy::{Copy, UiLocale},
  model_setup::{ModelSetupCommandState, ModelSetupDispatch, ModelSetupIntent},
};

const MAX_COMMAND_ERROR_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConnectionState {
  #[default]
  Disconnected,
  Connecting,
  Connected,
  Reconnecting,
  Unavailable,
  Incompatible,
}

#[derive(Clone, Debug, Default)]
pub struct CockpitState {
  runtime_details_expanded: bool,
  connection: ConnectionState,
  server_instance_id: Option<String>,
  revision: Option<u64>,
  stale: bool,
  runtime_phase: RuntimePhase,
  model_setup: ModelSetupCommandState,
  last_connection_error: Option<String>,
  submitting: bool,
  last_command_error: Option<String>,
  locale: UiLocale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDetailsPlacement {
  Hidden,
  Conversation,
  RuntimeRail,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubmissionDispatch {
  pub expected_server_instance_id: String,
  pub command: AgentCommand,
}

impl RuntimeDetailsPlacement {
  pub fn for_layout(compact: bool, expanded: bool) -> Self {
    match (compact, expanded) {
      (_, false) => Self::Hidden,
      (true, true) => Self::Conversation,
      (false, true) => Self::RuntimeRail,
    }
  }
}

impl CockpitState {
  pub fn connection(&self) -> ConnectionState {
    self.connection
  }

  pub fn server_instance_id(&self) -> Option<&str> {
    self.server_instance_id.as_deref()
  }

  pub fn revision(&self) -> Option<u64> {
    self.revision
  }

  pub fn is_stale(&self) -> bool {
    self.stale
  }

  pub fn last_connection_error(&self) -> Option<&str> {
    self.last_connection_error.as_deref()
  }

  pub fn can_submit(&self) -> bool {
    self.has_fresh_snapshot()
      && self.runtime_phase != RuntimePhase::ModelSetupRequired
      && !self.submitting
  }

  pub fn has_fresh_snapshot(&self) -> bool {
    self.connection == ConnectionState::Connected && self.revision.is_some() && !self.stale
  }

  pub fn is_submitting(&self) -> bool {
    self.submitting
  }

  pub fn last_command_error(&self) -> Option<&str> {
    self.last_command_error.as_deref()
  }

  pub fn begin_submission(&mut self, prompt: &str) -> Option<SubmissionDispatch> {
    let prompt = prompt.trim();
    if prompt.is_empty() || !self.can_submit() {
      return None;
    }
    let expected_server_instance_id = self.server_instance_id.clone()?;
    self.submitting = true;
    self.last_command_error = None;
    Some(SubmissionDispatch {
      expected_server_instance_id,
      command: AgentCommand::StartTurn {
        text: prompt.to_owned(),
      },
    })
  }

  pub fn finish_submission(&mut self, error: Option<&str>) {
    self.submitting = false;
    self.last_command_error = error.map(sanitize_command_error);
  }

  pub fn is_model_setup_required(&self) -> bool {
    self.runtime_phase == RuntimePhase::ModelSetupRequired
  }

  pub fn runtime_phase(&self) -> RuntimePhase {
    self.runtime_phase
  }

  pub fn model_setup(&self) -> Option<&ModelSetupView> {
    self.model_setup.view()
  }

  pub fn apply_runtime_snapshot(&mut self, snapshot: &RuntimeSnapshot) {
    self.runtime_phase = snapshot.phase;
    self.model_setup.sync_view(snapshot.model_setup.clone());
  }

  pub fn begin_model_setup_command(
    &mut self,
    intent: ModelSetupIntent,
  ) -> Option<ModelSetupDispatch> {
    let expected_server_instance_id = self
      .has_fresh_snapshot()
      .then(|| self.server_instance_id.clone())
      .flatten();
    self.model_setup.begin(expected_server_instance_id, intent)
  }

  pub fn finish_model_setup_command(&mut self, token: u64, error: Option<&str>) -> bool {
    self.model_setup.finish(token, error)
  }

  pub fn model_setup_command_pending(&self) -> bool {
    self.model_setup.is_pending()
  }

  pub fn model_setup_command_error(&self) -> Option<&str> {
    self.model_setup.error()
  }

  pub fn apply_connection(&mut self, connection: ConnectionState, error: Option<String>) {
    self.connection = connection;
    if connection != ConnectionState::Connected && self.revision.is_some() {
      self.stale = true;
    }
    self.last_connection_error = match connection {
      ConnectionState::Reconnecting
      | ConnectionState::Unavailable
      | ConnectionState::Incompatible => error,
      ConnectionState::Disconnected | ConnectionState::Connecting | ConnectionState::Connected => {
        None
      }
    };
  }

  pub fn apply_snapshot_revision(&mut self, server_instance_id: &str, revision: u64) -> bool {
    let same_instance = self.server_instance_id.as_deref() == Some(server_instance_id);
    let accept = if same_instance {
      match self.revision {
        Some(current) if revision > current => true,
        Some(current) if revision == current => self.stale,
        Some(_) => false,
        None => true,
      }
    } else {
      true
    };
    if !accept {
      return false;
    }

    if !same_instance {
      self.model_setup.reset_epoch();
    }
    self.server_instance_id = Some(server_instance_id.to_owned());
    self.revision = Some(revision);
    self.stale = self.connection != ConnectionState::Connected;
    true
  }

  pub(crate) fn mark_snapshot_stale(&mut self) {
    self.stale = self.revision.is_some();
  }

  pub fn runtime_details_expanded(&self) -> bool {
    self.runtime_details_expanded
  }

  pub fn runtime_details_control_label(&self) -> &'static str {
    let copy = Copy::new(self.locale);
    if self.runtime_details_expanded {
      copy.text("Hide details", "收起详情")
    } else {
      copy.text("Show details", "显示详情")
    }
  }

  pub fn toggle_runtime_details(&mut self) {
    self.runtime_details_expanded = !self.runtime_details_expanded;
  }

  pub fn locale(&self) -> UiLocale {
    self.locale
  }

  pub fn toggle_locale(&mut self) {
    self.locale = self.locale.toggled();
  }
}

fn sanitize_command_error(error: &str) -> String {
  let sanitized: String = error
    .chars()
    .map(|character| {
      if character.is_control() {
        ' '
      } else {
        character
      }
    })
    .take(MAX_COMMAND_ERROR_CHARS)
    .collect();
  let sanitized = sanitized.trim();
  sanitized.to_owned()
}
