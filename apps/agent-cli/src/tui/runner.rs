use std::{sync::mpsc, time::Duration};

use agent_kernel::Kernel;
use agent_tool::ApprovalDecision;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
  ApprovalRequest, TuiState,
  terminal::TerminalSession,
  ui::{RenderMetrics, render_cockpit},
};

pub use super::input::{edit_input, next_conversation_scroll};

type TurnTask = JoinHandle<(
  Kernel,
  Result<Vec<agent_core::AgentEvent>, agent_core::AgentError>,
)>;

pub struct Runtime {
  pub state: TuiState,
  pub kernel: Option<Kernel>,
  pub events: Option<broadcast::Receiver<agent_core::AgentEvent>>,
  pub task: Option<TurnTask>,
  pub approval: Option<ApprovalRequest>,
  pub approvals: Option<mpsc::Receiver<ApprovalRequest>>,
  pub approval_scroll: u16,
  pub approval_max_scroll: u16,
  pub conversation_scroll: u16,
  pub conversation_max_scroll: u16,
}

pub async fn run(kernel: Kernel, approvals: mpsc::Receiver<ApprovalRequest>) -> Result<()> {
  let cancellation = kernel.cancellation_token();
  let mut terminal = match TerminalSession::enter() {
    Ok(terminal) => terminal,
    Err(error) => {
      cancellation.cancel();
      return Err(error).context("failed to initialize terminal interface");
    }
  };
  let mut runtime = Runtime {
    state: TuiState::default(),
    kernel: Some(kernel),
    events: None,
    task: None,
    approval: None,
    approvals: Some(approvals),
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
  };

  let result = run_event_loop(&mut terminal, &mut runtime, &cancellation).await;
  let close = shutdown(&mut runtime, &cancellation).await;
  result?;
  close
}

async fn run_event_loop(
  terminal: &mut TerminalSession,
  runtime: &mut Runtime,
  cancellation: &CancellationToken,
) -> Result<()> {
  let mut cancel_requested = false;
  loop {
    drain_events(&mut runtime.events, &mut runtime.state);
    if runtime.approval.is_none()
      && let Some(approvals) = runtime.approvals.as_ref()
      && let Ok(request) = approvals.try_recv()
    {
      runtime.approval = Some(request);
      runtime.approval_scroll = 0;
    }
    if runtime.task.as_ref().is_some_and(JoinHandle::is_finished) {
      let finished = runtime.task.take().expect("finished task exists").await;
      final_drain(&mut runtime.events, &mut runtime.state);
      match finished {
        Ok((returned_kernel, Ok(_))) => runtime.kernel = Some(returned_kernel),
        Ok((returned_kernel, Err(error))) => {
          runtime.kernel = Some(returned_kernel);
          runtime.state.set_error(error.to_string());
        }
        Err(error) => {
          runtime
            .state
            .set_error(format!("turn task failed: {error}"));
          anyhow::bail!("turn task failed: {error}");
        }
      }
      runtime.events = None;
      if cancel_requested {
        return Ok(());
      }
    }
    runtime.state.busy = runtime.task.is_some();

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
        if runtime.task.is_some() {
          if cancel_requested {
            if let Some(task) = runtime.task.take() {
              task.abort();
              let _ = task.await;
            }
            return Ok(());
          }
          if let Some(request) = runtime.approval.take() {
            request.respond(ApprovalDecision::Deny);
          }
          cancellation.cancel();
          cancel_requested = true;
          continue;
        }
        return Ok(());
      }
      if quit_key(key) {
        return Ok(());
      }
      if runtime.approval.is_some() {
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
          runtime
            .approval
            .take()
            .expect("approval exists")
            .respond(decision);
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
      if !can_accept_input(runtime.task.is_some(), runtime.kernel.is_some()) {
        continue;
      }
      match key.code {
        KeyCode::Enter => {
          let prompt = runtime.state.input.trim().to_owned();
          if !prompt.is_empty() {
            runtime.state.input.clear();
            runtime.state.input_cursor = 0;
            runtime.state.mark_submitted();
            let owned_kernel = runtime
              .kernel
              .take()
              .context("kernel unavailable between turns")?;
            runtime.events = Some(owned_kernel.subscribe());
            runtime.task = Some(tokio::spawn(async move {
              let mut owned_kernel = owned_kernel;
              let result = owned_kernel.run_turn(&prompt).await;
              (owned_kernel, result)
            }));
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

pub async fn shutdown(runtime: &mut Runtime, cancellation: &CancellationToken) -> Result<()> {
  if let Some(request) = runtime.approval.take() {
    request.respond(ApprovalDecision::Deny);
  }
  runtime.approvals.take();
  cancellation.cancel();

  if let Some(mut task) = runtime.task.take() {
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
      Ok(Ok((kernel, _))) => runtime.kernel = Some(kernel),
      Ok(Err(_)) => {}
      Err(_) => {
        task.abort();
        let _ = task.await;
      }
    }
    final_drain(&mut runtime.events, &mut runtime.state);
  }
  if let Some(kernel) = runtime.kernel.as_mut() {
    kernel.close_storage().await?;
  }
  Ok(())
}

pub fn can_accept_input(task_active: bool, kernel_available: bool) -> bool {
  !task_active && kernel_available
}

pub fn final_drain(
  receiver: &mut Option<broadcast::Receiver<agent_core::AgentEvent>>,
  state: &mut TuiState,
) {
  drain_events(receiver, state);
}

fn drain_events(
  receiver: &mut Option<broadcast::Receiver<agent_core::AgentEvent>>,
  state: &mut TuiState,
) {
  let Some(receiver) = receiver else { return };
  loop {
    match receiver.try_recv() {
      Ok(event) => state.reduce(event),
      Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
        break;
      }
      Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
        state.set_error(format!("event stream lagged by {skipped} events"));
      }
    }
  }
}

pub fn quit_key(key: KeyEvent) -> bool {
  key.code == KeyCode::Esc
}

pub fn control_c_key(key: KeyEvent) -> bool {
  key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub fn approval_decision(key: KeyEvent) -> Option<ApprovalDecision> {
  if key.modifiers != KeyModifiers::NONE && key.modifiers != KeyModifiers::SHIFT {
    return None;
  }
  let KeyCode::Char(character) = key.code else {
    return None;
  };
  match character.to_ascii_lowercase() {
    'y' => Some(ApprovalDecision::Allow),
    'n' => Some(ApprovalDecision::Deny),
    'a' => Some(ApprovalDecision::AllowSession),
    'd' => Some(ApprovalDecision::DenyAll),
    _ => None,
  }
}
