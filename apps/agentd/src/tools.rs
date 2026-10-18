use crate::Daemon;
use agent_core::Run;
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_workspace::Workspace;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
struct WorkspaceTool {
  daemon: Daemon,
  run_id: String,
  name: &'static str,
  owner_epoch: u64,
  workspace: Workspace,
}
pub fn workspace_tools(
  daemon: Daemon,
  run: &Run,
  path: &str,
  owner_epoch: u64,
) -> anyhow::Result<Vec<Arc<dyn RuntimeTool>>> {
  let workspace = Workspace::open(path)?;
  Ok(
    ["read_file", "write_file"]
      .into_iter()
      .map(|name| {
        Arc::new(WorkspaceTool {
          daemon: daemon.clone(),
          run_id: run.id.clone(),
          name,
          owner_epoch,
          workspace: workspace.clone(),
        }) as Arc<dyn RuntimeTool>
      })
      .collect(),
  )
}
#[async_trait::async_trait]
impl RuntimeTool for WorkspaceTool {
  fn definition(&self) -> ToolDefinition {
    let (description, properties, required) = if self.name == "read_file" {
      (
        "Read a UTF-8 file relative to the authorized workspace, at most 1 MiB.",
        json!({"path":{"type":"string"}}),
        json!(["path"]),
      )
    } else {
      (
        "Write a UTF-8 file in the authorized workspace after exact-action human approval.",
        json!({"path":{"type":"string"},"content":{"type":"string"}}),
        json!(["path", "content"]),
      )
    };
    ToolDefinition {
      name: self.name.into(),
      description: description.into(),
      parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
  }
  async fn execute(&self, call: ToolCall, cancellation: CancellationToken) -> Result<ToolOutput> {
    let outcome = self.perform(&call.arguments, cancellation).await;
    match outcome {
      Ok(text) => Ok(ToolOutput {
        text,
        is_error: false,
      }),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        Err(error) => Ok(ToolOutput {
          text: error.to_string(),
          is_error: true,
        }),
      },
    }
  }
}
impl WorkspaceTool {
  async fn perform(&self, args: &Value, cancel: CancellationToken) -> anyhow::Result<String> {
    anyhow::ensure!(!cancel.is_cancelled(), "tool cancelled");
    let path = args
      .get("path")
      .and_then(Value::as_str)
      .ok_or_else(|| anyhow::anyhow!("path is required"))?
      .to_owned();
    if self.name == "read_file" {
      let workspace = self.workspace.clone();
      return tokio::task::spawn_blocking(move || workspace.read(&path)).await?;
    }
    let content = args
      .get("content")
      .and_then(Value::as_str)
      .ok_or_else(|| anyhow::anyhow!("content is required"))?
      .to_owned();
    crate::authorization::approve(
      &self.daemon,
      &self.run_id,
      self.owner_epoch,
      self.name,
      args,
      cancel.clone(),
    )
    .await?;
    anyhow::ensure!(!cancel.is_cancelled(), "tool cancelled before write");
    let workspace = self.workspace.clone();
    let _write = self.daemon.workspace_writes.lock().await;
    tokio::task::spawn_blocking(move || workspace.write(&path, &content))
      .await?
      .map_err(|e| {
        RuntimeError::Uncertain(format!("file write outcome requires verification: {e}"))
      })?;
    self.daemon.events.notify_waiters();
    Ok("File written.".into())
  }
}
