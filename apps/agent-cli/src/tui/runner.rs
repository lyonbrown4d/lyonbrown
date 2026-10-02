use std::time::Duration;

use agent_app::AgentCommand;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use super::{
  UiIntent,
  terminal::TerminalSession,
  ui::{RenderMetrics, render_cockpit},
  ui_intent,
};

pub use super::input::{edit_input, next_conversation_scroll};
pub use commands::{
  ComposerIntent, approval_decision, approval_interrupt_key, can_accept_input, composer_intent,
  control_c_key, quit_key,
};
use control::execute_control;
pub use events::final_drain;
pub use model_setup::{
  ModelSetupCommandGate, ModelSetupCompletion, ModelSetupExit, ModelSetupInput, model_setup_input,
};
pub use remote::sync_runtime_snapshot;

mod approval_input;
mod commands;
mod control;
mod dispatch;
mod events;
mod language;
mod local_commands;
mod model_setup;
mod remote;
mod session;

pub use approval_input::{ApprovalInputOutcome, handle_approval_interrupt};
pub use control::{
  ApprovalInterruptOutcome, CommandAttempt, execute_approval_interrupt_for_server_with_timeout,
  execute_control_for_server_with_timeout,
};
pub use dispatch::{CommandCompletion, CommandDispatcher, apply_command_completion};
use dispatch::{dispatch, dispatch_model_setup};
pub use language::{LanguageCommand, language_toggle_key};
use local_commands::LocalCommandResult::{Exit, Handled, NotCommand, Queue};
pub use local_commands::{LocalCommandResult, language_command};
pub use session::{Runtime, run_client, run_native_client, shutdown};

async fn run_event_loop(terminal: &mut TerminalSession, runtime: &mut Runtime) -> Result<()> {
  let mut cancel_requested = false;
  let mut commands = CommandDispatcher::new();
  let mut setup_commands = ModelSetupCommandGate::default();
  loop {
    sync_runtime_snapshot(runtime);
    if let Some(server_instance_id) = runtime.state.accepted_server_instance_id() {
      setup_commands.observe_server_epoch(server_instance_id);
    }
    if let Some(setup) = runtime.state.model_setup() {
      setup_commands.observe_snapshot(setup);
    }
    for completion in commands.drain_completions() {
      if apply_command_completion(&mut runtime.state, &mut setup_commands, completion)
        == ModelSetupCompletion::Exit
      {
        return Ok(());
      }
    }
    if !runtime.state.is_busy() {
      cancel_requested = false;
    }

    let mut metrics = RenderMetrics::default();
    let mut viewport_height = 0;
    terminal.terminal.draw(|frame| {
      viewport_height = frame.area().height;
      metrics = render_cockpit(
        frame,
        &runtime.state,
        runtime.approval.as_ref(),
        runtime.approval_scroll,
        runtime.conversation_scroll,
        runtime.render_context(setup_commands.is_pending(), setup_commands.is_closing()),
      );
    })?;
    runtime.approval_max_scroll = metrics.approval_max_scroll;
    runtime.approval_scroll = runtime.approval_scroll.min(metrics.approval_max_scroll);
    runtime.conversation_max_scroll = metrics.conversation_max_scroll;
    runtime.conversation_scroll = runtime
      .conversation_scroll
      .min(metrics.conversation_max_scroll);

    if event::poll(Duration::from_millis(25))?
      && let Event::Key(key) = event::read()?
    {
      if key.kind != KeyEventKind::Press {
        continue;
      }
      if language_toggle_key(key) {
        runtime.apply_language_command(LanguageCommand::Toggle);
        continue;
      }
      if let Some(setup) = runtime.state.model_setup() {
        let expected_server_instance_id = runtime
          .state
          .accepted_server_instance_id()
          .map(str::to_owned);
        let intent = setup_commands.input(setup, runtime.state.can_issue_setup_commands(), key);
        match intent {
          ModelSetupInput::Exit => {
            if setup_commands.request_exit() == ModelSetupExit::ExitNow {
              return Ok(());
            }
          }
          ModelSetupInput::Dispatch(command) => {
            let expected_server_instance_id = expected_server_instance_id
              .context("model setup command requires an authoritative daemon epoch")?;
            let token = setup_commands
              .pending_token()
              .expect("dispatch has a token");
            if let Err(error) = dispatch_model_setup(
              runtime,
              &mut commands,
              expected_server_instance_id,
              command,
              token,
            ) {
              setup_commands.complete(token, false);
              return Err(error);
            }
          }
          ModelSetupInput::Ignore => {}
        }
        continue;
      }
      match approval_input::handle(runtime, &mut commands, key, &mut cancel_requested).await? {
        ApprovalInputOutcome::NotHandled => {}
        ApprovalInputOutcome::Handled => continue,
        ApprovalInputOutcome::Exit => return Ok(()),
      }
      if runtime.state.overlay().is_open() {
        if let UiIntent::Activate(command) = runtime.state.apply_overlay_key(key, viewport_height)
          && runtime.apply_command_id(command) == Exit
        {
          return Ok(());
        }
        continue;
      }
      match ui_intent(key, runtime.state.input().is_empty(), false) {
        UiIntent::OpenPalette => {
          runtime.state.open_palette();
          continue;
        }
        UiIntent::OpenHelp => {
          runtime.state.open_help();
          continue;
        }
        _ => {}
      }
      if control_c_key(key) {
        if runtime.state.is_busy() {
          if cancel_requested {
            match execute_control(runtime, AgentCommand::ForceStop).await {
              Ok(()) => return Ok(()),
              Err(error) => runtime.state.set_error(error.to_string()),
            }
            cancel_requested = false;
            continue;
          }
          match execute_control(runtime, AgentCommand::CancelTurn).await {
            Ok(()) => cancel_requested = true,
            Err(error) => runtime.state.set_error(error.to_string()),
          }
          continue;
        }
        return Ok(());
      }
      if quit_key(key) {
        return Ok(());
      }
      let next_scroll = next_conversation_scroll(
        runtime.conversation_scroll,
        runtime.conversation_max_scroll,
        key.code,
      );
      if next_scroll != runtime.conversation_scroll {
        runtime.conversation_scroll = next_scroll;
        continue;
      }
      let task_active = runtime.state.is_busy();
      let daemon_input_enabled = can_accept_input(
        task_active,
        runtime.client.is_some() && runtime.state.can_submit(),
      );
      match key.code {
        KeyCode::Enter => {
          let prompt = runtime.state.input.trim().to_owned();
          if !prompt.is_empty() {
            match runtime.apply_slash_input(&prompt) {
              Handled => {
                runtime.state.input.clear();
                runtime.state.input_cursor = 0;
                continue;
              }
              Exit => return Ok(()),
              Queue(queued) => {
                if !daemon_input_enabled {
                  runtime.set_daemon_required_notice();
                  continue;
                }
                runtime.state.input.clear();
                runtime.state.input_cursor = 0;
                dispatch(
                  runtime,
                  &mut commands,
                  AgentCommand::QueueFollowUp { text: queued },
                )?;
                continue;
              }
              NotCommand if !daemon_input_enabled => {
                runtime.set_daemon_required_notice();
                continue;
              }
              NotCommand => {
                runtime.state.input.clear();
                runtime.state.input_cursor = 0;
              }
            }
            match composer_intent(task_active, key.modifiers) {
              ComposerIntent::StartTurn => {
                runtime.state.mark_submitted();
                dispatch(
                  runtime,
                  &mut commands,
                  AgentCommand::StartTurn { text: prompt },
                )?;
              }
              ComposerIntent::SteerCurrentTurn => {
                dispatch(
                  runtime,
                  &mut commands,
                  AgentCommand::SteerCurrentTurn { text: prompt },
                )?;
                runtime.state.mark_steering_submitted();
              }
              ComposerIntent::QueueFollowUp => {
                dispatch(
                  runtime,
                  &mut commands,
                  AgentCommand::QueueFollowUp {
                    text: prompt.clone(),
                  },
                )?;
              }
            }
          }
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Delete
        | KeyCode::Backspace
        | KeyCode::Char(_)
          if !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
          runtime.state.apply_input_key(key.code);
        }
        _ => {}
      }
    }
  }
}
