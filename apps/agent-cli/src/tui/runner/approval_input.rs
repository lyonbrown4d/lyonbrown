use agent_app::AgentCommand;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use super::{
  CommandDispatcher, Runtime,
  commands::{approval_decision, approval_interrupt_key, map_approval_decision},
  control::{execute_approval_interrupt, execute_control},
  dispatch::dispatch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalInputOutcome {
  NotHandled,
  Handled,
  Exit,
}

pub(super) async fn handle(
  runtime: &mut Runtime,
  commands: &mut CommandDispatcher,
  key: KeyEvent,
  cancel_requested: &mut bool,
) -> Result<ApprovalInputOutcome> {
  let Some(request_id) = runtime
    .approval
    .as_ref()
    .map(|approval| approval.request_id)
  else {
    return Ok(ApprovalInputOutcome::NotHandled);
  };
  match handle_approval_interrupt(runtime, key, cancel_requested).await? {
    ApprovalInputOutcome::NotHandled => {}
    outcome => return Ok(outcome),
  }
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
      commands,
      AgentCommand::RespondApproval {
        request_id,
        decision: map_approval_decision(decision),
      },
    )?;
  }
  Ok(ApprovalInputOutcome::Handled)
}

pub async fn handle_approval_interrupt(
  runtime: &mut Runtime,
  key: KeyEvent,
  cancel_requested: &mut bool,
) -> Result<ApprovalInputOutcome> {
  let Some(request_id) = runtime
    .approval
    .as_ref()
    .map(|approval| approval.request_id)
  else {
    return Ok(ApprovalInputOutcome::NotHandled);
  };
  if !approval_interrupt_key(key) {
    return Ok(ApprovalInputOutcome::NotHandled);
  }
  if *cancel_requested {
    match execute_control(runtime, AgentCommand::ForceStop).await {
      Ok(()) => return Ok(ApprovalInputOutcome::Exit),
      Err(error) => runtime.state.set_client_local_notice(error.to_string()),
    }
    return Ok(ApprovalInputOutcome::Handled);
  }
  if !runtime.state.can_submit() {
    return Ok(ApprovalInputOutcome::Handled);
  }
  match execute_approval_interrupt(runtime, request_id).await {
    Ok(outcome) => {
      *cancel_requested = outcome.cancel_succeeded();
      if let Some(notice) = outcome.failure_notice() {
        runtime.state.set_client_local_notice(notice);
      } else {
        runtime.state.clear_client_local_notice();
      }
    }
    Err(error) => runtime.state.set_client_local_notice(error.to_string()),
  }
  Ok(ApprovalInputOutcome::Handled)
}
