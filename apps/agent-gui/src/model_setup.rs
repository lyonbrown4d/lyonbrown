use agent_app::{
  AgentCommand, ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView,
};

pub use crate::copy::UiLocale;

mod presentation;
pub use presentation::{
  ModelSetupControl, ModelSetupDetail, ModelSetupDetailKind, ModelSetupPresentation,
  present_model_setup, present_model_setup_for_runtime,
};

const MAX_DISPLAY_TEXT_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelSetupIntent {
  Start,
  Retry,
  Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelSetupDispatch {
  pub token: u64,
  pub expected_server_instance_id: String,
  pub command: AgentCommand,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ModelSetupCommandState {
  view: Option<ModelSetupView>,
  next_token: u64,
  pending: Option<PendingCommand>,
  error: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingCommand {
  token: u64,
  intent: ModelSetupIntent,
  operation_id: Option<String>,
}

impl ModelSetupCommandState {
  pub fn view(&self) -> Option<&ModelSetupView> {
    self.view.as_ref()
  }

  pub fn sync_view(&mut self, view: Option<ModelSetupView>) {
    if logical_setup_changed(self.view.as_ref(), view.as_ref()) {
      self.error = None;
    }
    if self
      .pending
      .as_ref()
      .is_some_and(|pending| command_has_advanced(pending, view.as_ref()))
    {
      self.pending = None;
    }
    self.view = view;
  }

  pub fn begin(
    &mut self,
    expected_server_instance_id: Option<String>,
    intent: ModelSetupIntent,
  ) -> Option<ModelSetupDispatch> {
    if self.pending.is_some() {
      return None;
    }
    let expected_server_instance_id = expected_server_instance_id?;
    let view = self.view.as_ref()?;
    let command = command_for(view, intent)?;
    self.next_token = self.next_token.wrapping_add(1).max(1);
    let token = self.next_token;
    self.pending = Some(PendingCommand {
      token,
      intent,
      operation_id: view.operation_id.clone(),
    });
    self.error = None;
    Some(ModelSetupDispatch {
      token,
      expected_server_instance_id,
      command,
    })
  }

  pub fn finish(&mut self, token: u64, error: Option<&str>) -> bool {
    if self.pending.as_ref().map(|pending| pending.token) != Some(token) {
      return false;
    }
    if let Some(error) = error {
      self.pending = None;
      self.error = Some(sanitize_command_error(error));
    }
    true
  }

  pub fn is_pending(&self) -> bool {
    self.pending.is_some()
  }

  pub fn error(&self) -> Option<&str> {
    self.error.as_deref()
  }

  pub fn reset_epoch(&mut self) {
    self.pending = None;
    self.error = None;
  }
}

fn logical_setup_changed(previous: Option<&ModelSetupView>, next: Option<&ModelSetupView>) -> bool {
  match (previous, next) {
    (Some(previous), Some(next)) => {
      previous.phase != next.phase || previous.operation_id != next.operation_id
    }
    (None, None) => false,
    _ => true,
  }
}

fn command_has_advanced(pending: &PendingCommand, view: Option<&ModelSetupView>) -> bool {
  let Some(view) = view else {
    return true;
  };
  if pending.intent == ModelSetupIntent::Cancel {
    return pending.operation_id != view.operation_id
      || command_for(view, ModelSetupIntent::Cancel).is_none();
  }
  command_for(view, pending.intent).is_none()
}

fn command_for(setup: &ModelSetupView, intent: ModelSetupIntent) -> Option<AgentCommand> {
  if setup.source == ModelSetupSource::ConfiguredPath {
    return None;
  }
  match intent {
    ModelSetupIntent::Start if setup.allowed_actions.contains(&ModelSetupAction::Download) => {}
    ModelSetupIntent::Retry if setup.allowed_actions.contains(&ModelSetupAction::Retry) => {}
    ModelSetupIntent::Cancel
      if setup.allowed_actions.contains(&ModelSetupAction::Cancel)
        && !matches!(
          setup.phase,
          ModelSetupPhase::Installing | ModelSetupPhase::Activating
        ) =>
    {
      return setup
        .operation_id
        .clone()
        .map(|operation_id| AgentCommand::CancelModelDownload { operation_id });
    }
    _ => return None,
  }
  setup
    .managed_model
    .as_ref()
    .map(|model| AgentCommand::StartModelDownload {
      model_id: model.id.clone(),
    })
}

pub(super) fn sanitize_display_text(value: &str) -> String {
  sanitize_text(value)
}

fn sanitize_command_error(value: &str) -> String {
  sanitize_text(value)
}

fn sanitize_text(value: &str) -> String {
  let sanitized: String = value
    .chars()
    .map(|character| {
      if character.is_control() {
        ' '
      } else {
        character
      }
    })
    .take(MAX_DISPLAY_TEXT_CHARS)
    .collect();
  let sanitized = sanitized.trim();
  sanitized.to_owned()
}
