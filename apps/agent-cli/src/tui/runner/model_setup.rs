use agent_app::{
  AgentCommand, ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelSetupInput {
  Exit,
  Dispatch(AgentCommand),
  Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSetupExit {
  ExitNow,
  WaitForSetup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSetupCompletion {
  Continue,
  Exit,
}

#[derive(Debug, Default)]
pub struct ModelSetupCommandGate {
  pending: Option<PendingCommand>,
  exit_after_setup: Option<u64>,
  server_instance_id: Option<String>,
  next_token: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupCommandKind {
  Start,
  Cancel,
}

#[derive(Debug, Clone)]
struct PendingCommand {
  token: u64,
  kind: SetupCommandKind,
  phase: ModelSetupPhase,
  operation_id: Option<String>,
}

impl ModelSetupCommandGate {
  pub fn input(
    &mut self,
    setup: &ModelSetupView,
    commands_enabled: bool,
    key: KeyEvent,
  ) -> ModelSetupInput {
    if self.exit_after_setup.is_some() {
      return ModelSetupInput::Ignore;
    }
    let input = model_setup_input(setup, commands_enabled && self.pending.is_none(), key);
    if let ModelSetupInput::Dispatch(command) = &input {
      self.next_token = self
        .next_token
        .checked_add(1)
        .expect("model setup command token space exhausted");
      let kind = match command {
        AgentCommand::CancelModelDownload { .. } => SetupCommandKind::Cancel,
        _ => SetupCommandKind::Start,
      };
      self.pending = Some(PendingCommand {
        token: self.next_token,
        kind,
        phase: setup.phase,
        operation_id: setup.operation_id.clone(),
      });
    }
    input
  }

  pub fn observe_server_epoch(&mut self, server_instance_id: &str) -> bool {
    let changed = self
      .server_instance_id
      .as_deref()
      .is_some_and(|current| current != server_instance_id);
    self.server_instance_id = Some(server_instance_id.to_owned());
    if changed {
      self.pending = None;
      self.exit_after_setup = None;
    }
    changed
  }

  pub fn observe_snapshot(&mut self, setup: &ModelSetupView) {
    let start_applied = self.pending.as_ref().is_some_and(|pending| {
      pending.kind == SetupCommandKind::Start
        && (start_phase_advanced(pending.phase, setup.phase)
          || setup
            .operation_id
            .as_ref()
            .is_some_and(|operation_id| Some(operation_id) != pending.operation_id.as_ref()))
    });
    if start_applied {
      self.pending = None;
    }
  }

  pub fn complete(&mut self, token: u64, succeeded: bool) -> ModelSetupCompletion {
    if self
      .pending
      .as_ref()
      .is_some_and(|pending| pending.token == token)
    {
      self.pending = None;
    }
    if self.exit_after_setup == Some(token) {
      self.exit_after_setup = None;
      return if succeeded {
        ModelSetupCompletion::Exit
      } else {
        ModelSetupCompletion::Continue
      };
    }
    ModelSetupCompletion::Continue
  }

  pub fn request_exit(&mut self) -> ModelSetupExit {
    if self.exit_after_setup.is_some() {
      return ModelSetupExit::WaitForSetup;
    }
    let Some(pending) = self.pending.as_ref() else {
      return ModelSetupExit::ExitNow;
    };
    self.exit_after_setup = Some(pending.token);
    ModelSetupExit::WaitForSetup
  }

  pub fn is_pending(&self) -> bool {
    self.pending.is_some()
  }

  pub fn pending_token(&self) -> Option<u64> {
    self.pending.as_ref().map(|pending| pending.token)
  }

  pub fn is_closing(&self) -> bool {
    self.exit_after_setup.is_some()
  }
}

fn start_phase_advanced(previous: ModelSetupPhase, current: ModelSetupPhase) -> bool {
  previous != current
    && matches!(
      current,
      ModelSetupPhase::WaitingForLock
        | ModelSetupPhase::Checking
        | ModelSetupPhase::Downloading
        | ModelSetupPhase::Verifying
        | ModelSetupPhase::Installing
        | ModelSetupPhase::Activating
        | ModelSetupPhase::Ready
    )
}

pub fn model_setup_input(
  setup: &ModelSetupView,
  commands_enabled: bool,
  key: KeyEvent,
) -> ModelSetupInput {
  if has_command_modifiers(key.modifiers) {
    return ModelSetupInput::Ignore;
  }
  if matches!(key.code, KeyCode::Char('q' | 'Q')) {
    return ModelSetupInput::Exit;
  }
  if key.code == KeyCode::Esc && exits_on_escape(setup.phase) {
    return ModelSetupInput::Exit;
  }
  if !commands_enabled || setup.source == ModelSetupSource::ConfiguredPath {
    return ModelSetupInput::Ignore;
  }

  if matches!(key.code, KeyCode::Char('c' | 'C') | KeyCode::Esc)
    && !matches!(
      setup.phase,
      ModelSetupPhase::Installing | ModelSetupPhase::Activating
    )
    && setup.allowed_actions.contains(&ModelSetupAction::Cancel)
    && let Some(operation_id) = setup.operation_id.as_ref()
  {
    return ModelSetupInput::Dispatch(AgentCommand::CancelModelDownload {
      operation_id: operation_id.clone(),
    });
  }

  let download_key = matches!(key.code, KeyCode::Enter | KeyCode::Char('d' | 'D'));
  let retry_key = download_key || matches!(key.code, KeyCode::Char('r' | 'R'));
  let may_start = (download_key && setup.allowed_actions.contains(&ModelSetupAction::Download))
    || (retry_key && setup.allowed_actions.contains(&ModelSetupAction::Retry));
  if may_start && let Some(model) = setup.managed_model.as_ref() {
    return ModelSetupInput::Dispatch(AgentCommand::StartModelDownload {
      model_id: model.id.clone(),
    });
  }

  ModelSetupInput::Ignore
}

fn exits_on_escape(phase: ModelSetupPhase) -> bool {
  matches!(
    phase,
    ModelSetupPhase::Required | ModelSetupPhase::Cancelled | ModelSetupPhase::Failed
  )
}

fn has_command_modifiers(modifiers: KeyModifiers) -> bool {
  modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}
