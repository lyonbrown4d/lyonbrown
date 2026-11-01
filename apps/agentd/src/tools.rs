mod input;
mod schema;
use crate::Daemon;
use agent_core::Run;
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_workspace::{Workspace, load_repository_instructions_for_paths};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
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
    ["read_file", "write_file", "list_files", "search_files"]
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
    schema::definition(self.name)
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
  fn fence(&self, cancel: &CancellationToken) -> anyhow::Result<()> {
    anyhow::ensure!(
      !cancel.is_cancelled(),
      "workspace tool cancelled before dispatch"
    );
    let run = self
      .daemon
      .store
      .run(&self.run_id)?
      .ok_or_else(|| anyhow::anyhow!("workspace run unavailable"))?;
    let session = self
      .daemon
      .store
      .session(&run.session_id)?
      .ok_or_else(|| anyhow::anyhow!("workspace session unavailable"))?;
    anyhow::ensure!(
      session.owner_node_id == self.daemon.node_id && session.owner_epoch == self.owner_epoch,
      "workspace session ownership changed"
    );
    anyhow::ensure!(
      !matches!(
        run.status.as_str(),
        "cancel_requested" | "cancelled" | "completed" | "failed" | "uncertain"
      ),
      "workspace run is no longer active"
    );
    Ok(())
  }
  async fn perform(&self, args: &Value, cancel: CancellationToken) -> anyhow::Result<String> {
    self.fence(&cancel)?;
    let workspace = self.workspace.clone();
    match self.name {
      "read_file" => {
        let input: input::Read = serde_json::from_value(args.clone())?;
        return tokio::task::spawn_blocking(move || {
          let (content, revision) = workspace.read_revision(&input.path)?;
          let instructions = load_repository_instructions_for_paths(&workspace, &[PathBuf::from(&input.path)])?;
          Ok(json!({"content": content, "revision": revision, "repository_instructions": instructions}).to_string())
        })
        .await?;
      }
      "list_files" => {
        let input: input::Discover = serde_json::from_value(args.clone())?;
        return tokio::task::spawn_blocking(move || {
          let listing = workspace.list_files(&input.path, input.max_depth, input.max_results)?;
          let mut paths = listing.entries.iter().map(|entry| PathBuf::from(&entry.path)).collect::<Vec<_>>();
          paths.push(PathBuf::from(if input.path.is_empty() { "." } else { &input.path }));
          let instructions = load_repository_instructions_for_paths(&workspace, &paths)?;
          Ok(json!({"entries":listing.entries,"report":listing.report,"repository_instructions":instructions}).to_string())
        })
        .await?;
      }
      "search_files" => {
        let input: input::Search = serde_json::from_value(args.clone())?;
        return tokio::task::spawn_blocking(move || {
          let search = workspace.search_files(&input.path, &input.query, input.max_depth, input.max_results)?;
          let mut paths = search.matches.iter().map(|entry| PathBuf::from(&entry.path)).collect::<Vec<_>>();
          paths.push(PathBuf::from(if input.path.is_empty() { "." } else { &input.path }));
          let instructions = load_repository_instructions_for_paths(&workspace, &paths)?;
          Ok(json!({"matches":search.matches,"report":search.report,"repository_instructions":instructions}).to_string())
        })
        .await?;
      }
      _ => {}
    }
    let input: input::Write = serde_json::from_value(args.clone())?;
    anyhow::ensure!(
      input.content.len() <= 1_048_576,
      "file content exceeds 1 MiB limit"
    );
    anyhow::ensure!(
      input.expected_revision.is_empty()
        || (input.expected_revision.len() == 64
          && input
            .expected_revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
      "expected_revision must be a lowercase SHA-256 revision, or empty for creation"
    );
    let check = workspace.clone();
    let scope = PathBuf::from(&input.path);
    tokio::task::spawn_blocking(move || {
      load_repository_instructions_for_paths(&check, &[scope]).map(|_| ())
    })
    .await??;
    crate::authorization::approve(
      &self.daemon,
      &self.run_id,
      self.owner_epoch,
      self.name,
      args,
      cancel.clone(),
    )
    .await?;
    let write = self.daemon.workspace_writes.clone().lock_owned().await;
    let tool = self.clone();
    self.fence(&cancel)?;
    // All daemon writes share this lock. Validate after approval and before any replacement.
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
      let _write = write;
      tool.fence(&cancel)?;
      load_repository_instructions_for_paths(&workspace, &[PathBuf::from(&input.path)])?;
      workspace.check_revision(&input.path, &input.expected_revision)?;
      tool.fence(&cancel)?;
      workspace
        .write(&input.path, &input.content)
        .map_err(|error| {
          RuntimeError::Uncertain(format!("file write outcome requires verification: {error}"))
            .into()
        })
    })
    .await??;
    self.daemon.events.notify_waiters();
    Ok("File written.".into())
  }
}
