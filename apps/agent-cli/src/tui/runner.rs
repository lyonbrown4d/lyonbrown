use std::time::Duration;

use agent_app::{
  AgentCommand, AppController, ApprovalDecision as AppApprovalDecision, ApprovalInbox, RuntimeTask,
  spawn_runtime, spawn_runtime_with_approvals,
};
use agent_kernel::Kernel;
use agent_tool::ApprovalDecision;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::watch;

use super::{
  ApprovalRequest, TuiState,
  terminal::TerminalSession,
  ui::{RenderMetrics, render_cockpit},
};

pub use super::input::{edit_input, next_conversation_scroll};
pub use commands::{
  ComposerIntent, approval_decision, can_accept_input, composer_intent, control_c_key, quit_key,
};
pub use events::final_drain;

mod commands;
mod events;

pub struct Runtime {
  pub state: TuiState,
  pub controller: Option<AppController>,
  pub snapshots: watch::Receiver<agent_app::RuntimeSnapshot>,
  pub actor_task: Option<RuntimeTask>,
  pub approval: Option<ApprovalRequest>,
  pub approval_scroll: u16,
  pub approval_max_scroll: u16,
  pub conversation_scroll: u16,
  pub conversation_max_scroll: u16,
}

pub async fn run(kernel: Kernel, approvals: Option<ApprovalInbox>) -> Result<()> {
  let (controller, actor_task) = match approvals {
    Some(approvals) => spawn_runtime_with_approvals(kernel, approvals, 32),
    None => spawn_runtime(kernel, 32),
  };
  let snapshots = controller.subscribe();
  let mut terminal = match TerminalSession::enter() {
    Ok(terminal) => terminal,
    Err(error) => {
      drop(controller);
      actor_task.join().await?;
      return Err(error).context("failed to initialize terminal interface");
    }
  };
  let mut runtime = Runtime {
    state: TuiState::default(),
    controller: Some(controller),
    snapshots,
    actor_task: Some(actor_task),
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
  loop {
    sync_runtime_snapshot(runtime);
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
            let _ = execute(runtime, AgentCommand::ForceStop).await;
            return Ok(());
          }
          execute(runtime, AgentCommand::CancelTurn).await?;
          cancel_requested = true;
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
        if let Some(decision) = approval_decision(key) {
          execute(
            runtime,
            AgentCommand::RespondApproval {
              request_id,
              decision: map_approval_decision(decision),
            },
          )
          .await?;
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
      if !can_accept_input(task_active, runtime.controller.is_some()) {
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
                execute(
                  runtime,
                  AgentCommand::QueueFollowUp {
                    text: queued.clone(),
                  },
                )
                .await?;
              }
              continue;
            }
            match composer_intent(task_active, key.modifiers) {
              ComposerIntent::StartTurn => {
                runtime.state.mark_submitted();
                execute(runtime, AgentCommand::StartTurn { text: prompt }).await?;
              }
              ComposerIntent::SteerCurrentTurn => {
                execute(runtime, AgentCommand::SteerCurrentTurn { text: prompt }).await?;
                runtime.state.mark_steering_submitted();
              }
              ComposerIntent::QueueFollowUp => {
                execute(
                  runtime,
                  AgentCommand::QueueFollowUp {
                    text: prompt.clone(),
                  },
                )
                .await?;
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
  if runtime.state.is_busy()
    && let Some(controller) = runtime.controller.as_ref()
  {
    let _ = controller.execute(AgentCommand::CancelTurn).await;
  }
  runtime.controller.take();
  if let Some(task) = runtime.actor_task.take() {
    task.join().await?;
  }
  Ok(())
}

fn sync_runtime_snapshot(runtime: &mut Runtime) {
  let snapshot = runtime.snapshots.borrow().clone();
  let next_approval = snapshot
    .approval
    .as_ref()
    .and_then(ApprovalRequest::from_view);
  if runtime.approval.as_ref().map(|request| request.request_id)
    != next_approval.as_ref().map(|request| request.request_id)
  {
    runtime.approval_scroll = 0;
  }
  runtime.approval = next_approval;
  runtime
    .state
    .set_queued_follow_up(snapshot.queued_follow_ups.first().cloned());
  runtime.state.sync_snapshot(&snapshot);
}

fn map_approval_decision(decision: ApprovalDecision) -> AppApprovalDecision {
  match decision {
    ApprovalDecision::Allow => AppApprovalDecision::AllowOnce,
    ApprovalDecision::AllowSession => AppApprovalDecision::AllowForSession,
    ApprovalDecision::Deny => AppApprovalDecision::DenyOnce,
    ApprovalDecision::DenyAll => AppApprovalDecision::DenyRemaining,
  }
}

async fn execute(runtime: &Runtime, command: AgentCommand) -> Result<()> {
  runtime
    .controller
    .as_ref()
    .context("application runtime is unavailable")?
    .execute(command)
    .await
    .map_err(anyhow::Error::from)
}
