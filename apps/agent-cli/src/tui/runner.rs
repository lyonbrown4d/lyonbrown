use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentClient, AgentCommand, ApprovalDecision as AppApprovalDecision};
use agent_client::{NativeAgentClient, RemoteState};
use agent_tool::ApprovalDecision;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::watch;

use super::{
  ApprovalRequest, TuiState, apply_remote_state,
  terminal::TerminalSession,
  ui::{RenderMetrics, render_cockpit},
};

pub use super::input::{edit_input, next_conversation_scroll};
pub use commands::{
  ComposerIntent, approval_decision, can_accept_input, composer_intent, control_c_key, quit_key,
};
pub use events::final_drain;

mod commands;
mod control;
mod dispatch;
mod events;
mod remote;

pub use control::execute_control_with_timeout;
pub use dispatch::CommandDispatcher;

const CONTROL_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Runtime {
  pub state: TuiState,
  pub client: Option<Arc<dyn AgentClient>>,
  pub snapshots: watch::Receiver<agent_app::RuntimeSnapshot>,
  pub remote_state: Option<watch::Receiver<RemoteState>>,
  pub approval: Option<ApprovalRequest>,
  pub approval_scroll: u16,
  pub approval_max_scroll: u16,
  pub conversation_scroll: u16,
  pub conversation_max_scroll: u16,
}

pub async fn run_client(client: Arc<dyn AgentClient>) -> Result<()> {
  let snapshots = client.subscribe();
  run_client_with_state(client, snapshots, None).await
}

pub async fn run_native_client(client: NativeAgentClient) -> Result<()> {
  let remote_state = client.subscribe_remote_state();
  let client: Arc<dyn AgentClient> = Arc::new(client);
  let snapshots = client.subscribe();
  run_client_with_state(client, snapshots, Some(remote_state)).await
}

async fn run_client_with_state(
  client: Arc<dyn AgentClient>,
  snapshots: watch::Receiver<agent_app::RuntimeSnapshot>,
  remote_state: Option<watch::Receiver<RemoteState>>,
) -> Result<()> {
  let mut state = TuiState::default();
  if let Some(remote_state) = remote_state.as_ref() {
    apply_remote_state(&mut state, &remote_state.borrow());
  }
  let mut terminal = match TerminalSession::enter() {
    Ok(terminal) => terminal,
    Err(error) => return Err(error).context("failed to initialize terminal interface"),
  };
  let mut runtime = Runtime {
    state,
    client: Some(client),
    snapshots,
    remote_state,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
  };

  let result = run_event_loop(&mut terminal, &mut runtime).await;
  let close = shutdown(&mut runtime).await;
  result?;
  close
}

async fn run_event_loop(terminal: &mut TerminalSession, runtime: &mut Runtime) -> Result<()> {
  let mut cancel_requested = false;
  let mut commands = CommandDispatcher::new();
  loop {
    for error in commands.drain_failures() {
      runtime.state.record_client_failure(error);
    }
    remote::sync_runtime_snapshot(runtime);
    if !runtime.state.is_busy() {
      cancel_requested = false;
    }

    let mut metrics = RenderMetrics::default();
    terminal.terminal.draw(|frame| {
      metrics = render_cockpit(
        frame,
        &runtime.state,
        runtime.approval.as_ref(),
        runtime.approval_scroll,
        runtime.conversation_scroll,
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
      if let Some(approval) = runtime.approval.as_ref() {
        let request_id = approval.request_id;
        match key.code {
          KeyCode::Up => runtime.approval_scroll = runtime.approval_scroll.saturating_sub(1),
          KeyCode::Down => {
            runtime.approval_scroll = runtime
              .approval_scroll
              .saturating_add(1)
              .min(runtime.approval_max_scroll)
          }
          KeyCode::PageUp => runtime.approval_scroll = runtime.approval_scroll.saturating_sub(5),
          KeyCode::PageDown => {
            runtime.approval_scroll = runtime
              .approval_scroll
              .saturating_add(5)
              .min(runtime.approval_max_scroll)
          }
          KeyCode::Home => runtime.approval_scroll = 0,
          KeyCode::End => runtime.approval_scroll = runtime.approval_max_scroll,
          _ => {}
        }
        if let Some(decision) = approval_decision(key)
          && runtime.state.can_submit()
        {
          dispatch(
            runtime,
            &mut commands,
            AgentCommand::RespondApproval {
              request_id,
              decision: map_approval_decision(decision),
            },
          )?;
        }
        continue;
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
      if !can_accept_input(
        task_active,
        runtime.client.is_some() && runtime.state.can_submit(),
      ) {
        continue;
      }
      match key.code {
        KeyCode::Enter => {
          let prompt = runtime.state.input.trim().to_owned();
          if !prompt.is_empty() {
            runtime.state.input.clear();
            runtime.state.input_cursor = 0;
            if prompt == "/quit" {
              return Ok(());
            }
            if let Some(queued) = prompt.strip_prefix("/queue ") {
              let queued = queued.trim().to_owned();
              if !queued.is_empty() {
                dispatch(
                  runtime,
                  &mut commands,
                  AgentCommand::QueueFollowUp {
                    text: queued.clone(),
                  },
                )?;
              }
              continue;
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

pub async fn shutdown(runtime: &mut Runtime) -> Result<()> {
  runtime.client.take();
  Ok(())
}

fn map_approval_decision(decision: ApprovalDecision) -> AppApprovalDecision {
  match decision {
    ApprovalDecision::Allow => AppApprovalDecision::AllowOnce,
    ApprovalDecision::AllowSession => AppApprovalDecision::AllowForSession,
    ApprovalDecision::Deny => AppApprovalDecision::DenyOnce,
    ApprovalDecision::DenyAll => AppApprovalDecision::DenyRemaining,
  }
}

fn dispatch(
  runtime: &Runtime,
  commands: &mut CommandDispatcher,
  command: AgentCommand,
) -> Result<()> {
  let client = runtime
    .client
    .as_ref()
    .context("application runtime is unavailable")?
    .clone();
  commands.dispatch(client, command);
  Ok(())
}

async fn execute_control(runtime: &Runtime, command: AgentCommand) -> Result<()> {
  let client = runtime
    .client
    .as_ref()
    .context("application runtime is unavailable")?;
  execute_control_with_timeout(client.as_ref(), command, CONTROL_COMMAND_TIMEOUT).await
}
