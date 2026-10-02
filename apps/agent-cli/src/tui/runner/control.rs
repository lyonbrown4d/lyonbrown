use std::time::Duration;

use agent_app::{AgentClient, AgentCommand};
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
