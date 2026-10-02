use std::time::Duration;

use agent_app::{AgentClient, AgentCommand};
use anyhow::Result;

pub async fn execute_control_with_timeout(
  client: &dyn AgentClient,
  command: AgentCommand,
  timeout: Duration,
) -> Result<()> {
  let action = match &command {
    AgentCommand::CancelTurn => "cancel turn",
    AgentCommand::ForceStop => "force-stop turn",
    _ => anyhow::bail!("timed execution only supports turn control commands"),
  };
  match tokio::time::timeout(timeout, client.execute(command)).await {
    Ok(Ok(())) => Ok(()),
    Ok(Err(error)) => Err(anyhow::anyhow!("{action} command failed: {error}")),
    Err(_) => Err(anyhow::anyhow!(
      "{action} command timed out after {}ms",
      timeout.as_millis()
    )),
  }
}
