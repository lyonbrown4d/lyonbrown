use std::time::Duration;

use agent_app::{AgentClient, AgentCommand, ApprovalDecision};
use agent_core::ToolCallId;
use anyhow::Result;

use super::session::Runtime;

const CONTROL_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn execute_control(runtime: &Runtime, command: AgentCommand) -> Result<()> {
  let client = runtime
    .client
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("application runtime is unavailable"))?;
  let expected_server_instance_id = runtime
    .state
    .accepted_server_instance_id()
    .ok_or_else(|| anyhow::anyhow!("turn control requires an authoritative daemon epoch"))?
    .to_owned();
  execute_control_for_server_with_timeout(
    client.as_ref(),
    &expected_server_instance_id,
    command,
    CONTROL_COMMAND_TIMEOUT,
  )
  .await
}

pub(super) async fn execute_approval_interrupt(
  runtime: &Runtime,
  request_id: ToolCallId,
) -> Result<ApprovalInterruptOutcome> {
  let client = runtime
    .client
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("application runtime is unavailable"))?;
  let expected_server_instance_id = runtime
    .state
    .accepted_server_instance_id()
    .ok_or_else(|| anyhow::anyhow!("approval interrupt requires an authoritative daemon epoch"))?;
  Ok(
    execute_approval_interrupt_for_server_with_timeout(
      client.as_ref(),
      expected_server_instance_id,
      request_id,
      CONTROL_COMMAND_TIMEOUT,
    )
    .await,
  )
}

pub async fn execute_approval_interrupt_for_server_with_timeout(
  client: &dyn AgentClient,
  expected_server_instance_id: &str,
  request_id: ToolCallId,
  timeout: Duration,
) -> ApprovalInterruptOutcome {
  let deny = execute_named_with_timeout(
    client,
    expected_server_instance_id,
    AgentCommand::RespondApproval {
      request_id,
      decision: ApprovalDecision::DenyOnce,
    },
    "deny approval",
    timeout,
  )
  .await;
  let cancel = execute_named_with_timeout(
    client,
    expected_server_instance_id,
    AgentCommand::CancelTurn,
    "cancel turn",
    timeout,
  )
  .await;
  ApprovalInterruptOutcome {
    deny: CommandAttempt::from_result(deny),
    cancel: CommandAttempt::from_result(cancel),
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAttempt {
  Succeeded,
  Failed(String),
}

impl CommandAttempt {
  fn from_result(result: Result<()>) -> Self {
    match result {
      Ok(()) => Self::Succeeded,
      Err(error) => Self::Failed(error.to_string()),
    }
  }

  const fn succeeded(&self) -> bool {
    matches!(self, Self::Succeeded)
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalInterruptOutcome {
  pub deny: CommandAttempt,
  pub cancel: CommandAttempt,
}

impl ApprovalInterruptOutcome {
  pub const fn cancel_succeeded(&self) -> bool {
    self.cancel.succeeded()
  }

  pub fn failure_notice(&self) -> Option<String> {
    match (&self.deny, &self.cancel) {
      (CommandAttempt::Succeeded, CommandAttempt::Succeeded) => None,
      (CommandAttempt::Failed(deny), CommandAttempt::Succeeded) => Some(deny.clone()),
      (CommandAttempt::Succeeded, CommandAttempt::Failed(cancel)) => Some(cancel.clone()),
      (CommandAttempt::Failed(deny), CommandAttempt::Failed(cancel)) => {
        Some(format!("{deny}; then {cancel}"))
      }
    }
  }
}

pub async fn execute_control_for_server_with_timeout(
  client: &dyn AgentClient,
  expected_server_instance_id: &str,
  command: AgentCommand,
  timeout: Duration,
) -> Result<()> {
  let action = match &command {
    AgentCommand::CancelTurn => "cancel turn",
    AgentCommand::ForceStop => "force-stop turn",
    _ => anyhow::bail!("timed execution only supports turn control commands"),
  };
  execute_named_with_timeout(
    client,
    expected_server_instance_id,
    command,
    action,
    timeout,
  )
  .await
}

async fn execute_named_with_timeout(
  client: &dyn AgentClient,
  expected_server_instance_id: &str,
  command: AgentCommand,
  action: &str,
  timeout: Duration,
) -> Result<()> {
  let execute = client.execute_for_server(expected_server_instance_id, command);
  match tokio::time::timeout(timeout, execute).await {
    Ok(Ok(())) => Ok(()),
    Ok(Err(error)) => Err(anyhow::anyhow!("{action} command failed: {error}")),
    Err(_) => Err(anyhow::anyhow!(
      "{action} command timed out after {}ms",
      timeout.as_millis()
    )),
  }
}
