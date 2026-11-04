//! Daemon-owned reviewed patch tools. The caller's effective tool allowlist still applies.
mod apply;
mod input;
mod schema;
mod user;
use crate::Daemon;
use agent_changes::{ChangeSet, MAX_BYTES, MAX_FILES, build_preview, reverse_preview};
use agent_core::{Run, Session};
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_workspace::{Workspace, load_repository_instructions_for_paths};
use anyhow::ensure;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct ChangeTool {
  daemon: Daemon,
  run_id: String,
  session_id: String,
  epoch: u64,
  workspace: Workspace,
  name: &'static str,
}
pub(crate) fn tools(
  daemon: Daemon,
  run: &Run,
  session: &Session,
) -> anyhow::Result<Vec<Arc<dyn RuntimeTool>>> {
  let workspace = Workspace::open(&session.workspace)?;
  Ok(
    [
      "patch_preview",
      "patch_apply",
      "checkpoint_read",
      "checkpoint_undo",
      "validation_record",
    ]
    .into_iter()
    .map(|name| {
      Arc::new(ChangeTool {
        daemon: daemon.clone(),
        run_id: run.id.clone(),
        session_id: session.id.clone(),
        epoch: session.owner_epoch,
        workspace: workspace.clone(),
        name,
      }) as Arc<dyn RuntimeTool>
    })
    .collect(),
  )
}
#[async_trait::async_trait]
impl RuntimeTool for ChangeTool {
  fn definition(&self) -> ToolDefinition {
    schema::definition(self.name)
  }
  async fn execute(&self, call: ToolCall, cancel: CancellationToken) -> Result<ToolOutput> {
    match self.perform(call.arguments, cancel).await {
      Ok(value) => Ok(ToolOutput {
        text: value.to_string(),
        is_error: false,
      }),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        Err(_) => Ok(ToolOutput {
          text: "change request rejected; check revisions, session ownership, approval and limits"
            .into(),
          is_error: true,
        }),
      },
    }
  }
}
impl ChangeTool {
  async fn fence(&self, cancel: &CancellationToken) -> anyhow::Result<()> {
    ensure!(!cancel.is_cancelled(), "change cancelled before dispatch");
    let run = self
      .daemon
      .store
      .run(&self.run_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("change run missing"))?;
    let session = self
      .daemon
      .store
      .session(&self.session_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("change session missing"))?;
    ensure!(
      run.session_id == self.session_id
        && run.status == "running"
        && session.owner_node_id == self.daemon.node_id
        && session.owner_epoch == self.epoch,
      "change ownership changed"
    );
    Ok(())
  }
  async fn checkpoint(&self, id: &str) -> anyhow::Result<ChangeSet> {
    let change = self.daemon.store.change_set(id).await?;
    let original = self
      .daemon
      .store
      .run(&change.run_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("checkpoint run missing"))?;
    ensure!(
      original.session_id == self.session_id,
      "checkpoint belongs to another session"
    );
    Ok(change)
  }
  async fn perform(&self, args: Value, cancel: CancellationToken) -> anyhow::Result<Value> {
    self.fence(&cancel).await?;
    match self.name {
      "patch_preview" => {
        let input: input::Preview = serde_json::from_value(args)?;
        ensure!(
          !input.files.is_empty() && input.files.len() <= MAX_FILES,
          "patch file limit exceeded"
        );
        ensure!(
          input.files.iter().map(|f| f.patch.len()).sum::<usize>() <= MAX_BYTES,
          "patch byte limit exceeded"
        );
        let workspace = self.workspace.clone();
        let run = self.run_id.clone();
        let epoch = self.epoch;
        let id = input.request_id.clone();
        let change = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
          let paths = input
            .files
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect::<Vec<_>>();
          load_repository_instructions_for_paths(&workspace, &paths)?;
          let mut files = Vec::with_capacity(input.files.len());
          for file in input.files {
            workspace.validate_change_path(&file.path)?;
            workspace.check_revision(&file.path, &file.expected_revision)?;
            let before = if file.expected_revision.is_empty() {
              None
            } else {
              Some(workspace.read_revision(&file.path)?.0)
            };
            files.push((file, before));
          }
          build_preview(id, run, epoch, files)
        })
        .await??;
        self.fence(&cancel).await?;
        let saved = self
          .daemon
          .store
          .save_change_preview(&input.request_id, change)
          .await?;
        Ok(serde_json::to_value(agent_changes::model_change(
          &saved,
          &[],
          false,
        ))?)
      }
      "patch_apply" => self.apply(serde_json::from_value(args)?, cancel).await,
      "checkpoint_undo" => {
        let input: input::Undo = serde_json::from_value(args)?;
        let applied = self.checkpoint(&input.change_id).await?;
        let reversed = reverse_preview(
          input.request_id.clone(),
          self.run_id.clone(),
          self.epoch,
          &applied,
        )?;
        let workspace = self.workspace.clone();
        let check = reversed.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
          for file in check.files {
            workspace.validate_change_path(&file.path)?;
            workspace.check_revision(&file.path, &file.expected_revision)?;
          }
          Ok(())
        })
        .await??;
        self.fence(&cancel).await?;
        let saved = self
          .daemon
          .store
          .save_change_preview(&input.request_id, reversed)
          .await?;
        Ok(serde_json::to_value(agent_changes::model_change(
          &saved,
          &[],
          false,
        ))?)
      }
      "validation_record" => {
        let input: input::Validation = serde_json::from_value(args)?;
        let _write = self.daemon.workspace_writes.clone().lock_owned().await;
        self.fence(&cancel).await?;
        let checkpoint = self.checkpoint(&input.change_id).await?;
        let workspace = self.workspace.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
          for file in checkpoint.files {
            workspace.validate_change_path(&file.path)?;
            workspace.check_revision(&file.path, file.after_sha256.as_deref().unwrap_or(""))?;
          }
          Ok(())
        })
        .await??;
        self.fence(&cancel).await?;
        Ok(serde_json::to_value(
          self
            .daemon
            .store
            .record_change_validation(
              &input.request_id,
              &input.change_id,
              &self.run_id,
              self.epoch,
              input.expected_revision,
              &input.operation_id,
            )
            .await?,
        )?)
      }
      _ => {
        let input: input::Read = serde_json::from_value(args)?;
        let change = self.checkpoint(&input.change_id).await?;
        let validations = self
          .daemon
          .store
          .recent_change_validations(
            &input.change_id,
            agent_changes::MODEL_VALIDATIONS as u32 + 1,
          )
          .await?;
        Ok(serde_json::to_value(agent_changes::model_change(
          &change,
          &validations,
          false,
        ))?)
      }
    }
  }
}
