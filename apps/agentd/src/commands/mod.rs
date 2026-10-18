//! Daemon-owned trusted native command tool with durable exact-action authorization.
mod authorization;
mod input;
mod schema;

use crate::{Daemon, config::Config};
use agent_core::{Run, Session};
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_storage::OperationStatus;
use agent_workspace::process::{CommandExecutor, CommandPolicy, CommandStatus};
use anyhow::ensure;
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct CommandTool {
  daemon: Daemon,
  run_id: String,
  session_id: String,
  workspace: String,
  epoch: u64,
  policy: CommandPolicy,
}

pub(crate) fn enabled(config: &Config) -> bool {
  !config.commands.executables.is_empty()
}

pub(crate) fn validate(config: &Config) -> anyhow::Result<()> {
  config.commands.validate()?;
  policy(config)?;
  Ok(())
}

fn policy(config: &Config) -> anyhow::Result<CommandPolicy> {
  CommandPolicy::new(
    config.commands.executables.iter().cloned(),
    config.command_environment.clone(),
  )
}

pub(crate) fn tools(
  daemon: Daemon,
  run: &Run,
  session: &Session,
) -> anyhow::Result<Vec<Arc<dyn RuntimeTool>>> {
  if !enabled(&daemon.config) {
    return Ok(Vec::new());
  }
  let policy = policy(&daemon.config)?;
  Ok(vec![Arc::new(CommandTool {
    daemon,
    run_id: run.id.clone(),
    session_id: session.id.clone(),
    workspace: session.workspace.clone(),
    epoch: session.owner_epoch,
    policy,
  })])
}

#[async_trait::async_trait]
impl RuntimeTool for CommandTool {
  fn definition(&self) -> ToolDefinition {
    schema::definition(&self.daemon.config.commands.executables)
  }

  async fn execute(&self, call: ToolCall, cancel: CancellationToken) -> Result<ToolOutput> {
    match self.perform(call, cancel).await {
      Ok(output) => Ok(output),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        Err(_) => Ok(ToolOutput {
          text: "command request could not be authorized or executed".into(),
          is_error: true,
        }),
      },
    }
  }
}

impl CommandTool {
  fn fence(&self, cancel: &CancellationToken) -> anyhow::Result<()> {
    ensure!(!cancel.is_cancelled(), "command cancelled before dispatch");
    let session = self
      .daemon
      .store
      .session(&self.session_id)?
      .ok_or_else(|| anyhow::anyhow!("command session unavailable"))?;
    ensure!(
      session.owner_node_id == self.daemon.node_id && session.owner_epoch == self.epoch,
      "command session ownership changed"
    );
    let run = self
      .daemon
      .store
      .run(&self.run_id)?
      .ok_or_else(|| anyhow::anyhow!("command run unavailable"))?;
    ensure!(
      run.session_id == self.session_id
        && !matches!(
          run.status.as_str(),
          "cancel_requested" | "cancelled" | "completed" | "failed" | "uncertain"
        ),
      "command run is no longer active"
    );
    Ok(())
  }

  async fn perform(&self, call: ToolCall, cancel: CancellationToken) -> anyhow::Result<ToolOutput> {
    self.fence(&cancel)?;
    let request = input::request(&self.run_id, &call)?;
    if self.daemon.store.operation(&request.action_id)?.is_some() {
      return Err(
        RuntimeError::Uncertain(
          "command action already recorded; inspect its durable receipt before acting again".into(),
        )
        .into(),
      );
    }
    let began = Arc::new(AtomicBool::new(false));
    let authorization = Arc::new(authorization::Authorization {
      tool: self.clone(),
      call_id: call.id,
      cancel: cancel.clone(),
      began: began.clone(),
    });
    let executor = CommandExecutor::open(&self.workspace, self.policy.clone(), authorization)?;
    let outcome = executor.execute(request, cancel).await;
    if began.load(Ordering::Acquire) {
      let status = match outcome.status {
        CommandStatus::Succeeded => OperationStatus::Completed,
        CommandStatus::Uncertain => OperationStatus::Uncertain,
        _ => OperationStatus::Failed,
      };
      self.daemon.store.finish_operation(&self.run_id, self.epoch, &outcome.action_id, status,
        json!({"reason":outcome.reason,"exit_code":outcome.exit_code,
          "output_truncated":outcome.output_truncated,"stdout_bytes":outcome.stdout.len(),"stderr_bytes":outcome.stderr.len()}))
        .map_err(|_| RuntimeError::Uncertain("command receipt could not be persisted; reconcile before acting again".into()))?;
      self.daemon.events.notify_waiters();
    } else if self.daemon.store.operation(&outcome.action_id)?.is_some() {
      return Err(
        RuntimeError::Uncertain(
          "command action is already recorded; automatic replay is forbidden".into(),
        )
        .into(),
      );
    }
    if outcome.status == CommandStatus::Uncertain {
      return Err(
        RuntimeError::Uncertain(format!(
          "command outcome is uncertain ({:?}); inspect action {} before acting again",
          outcome.reason, outcome.action_id
        ))
        .into(),
      );
    }
    Ok(ToolOutput {
      text: json!({"untrusted_process_output":true,"outcome":outcome}).to_string(),
      is_error: outcome.status != CommandStatus::Succeeded,
    })
  }
}
