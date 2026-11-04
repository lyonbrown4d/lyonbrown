use super::{ChangeTool, input};
use agent_changes::ChangeState;
use agent_runtime::RuntimeError;
use agent_storage::changes::PrepareChange;
use agent_workspace::load_repository_instructions_for_paths;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

impl ChangeTool {
  pub(super) async fn apply(
    &self,
    input: input::Apply,
    cancel: CancellationToken,
  ) -> Result<Value> {
    let change = self.checkpoint(&input.change_id).await?;
    ensure!(
      change.run_id == self.run_id
        && change.owner_epoch == self.epoch
        && change.state == ChangeState::Preview
        && change.revision == input.expected_revision
        && change.preview_sha256 == input.preview_sha256,
      "change preview mismatch"
    );
    let exact = json!({"request_id":input.request_id,"change_id":input.change_id,
      "expected_revision":input.expected_revision,"preview_sha256":input.preview_sha256,
      "run_id":self.run_id,"owner_epoch":self.epoch,"action_id":input.action_id});
    let approval = crate::authorization::wait_for_approval(
      &self.daemon,
      &self.run_id,
      self.epoch,
      "patch_apply",
      &exact,
      cancel.clone(),
    )
    .await?;
    let write = self.daemon.workspace_writes.clone().lock_owned().await;
    self.fence(&cancel).await?;
    let workspace = self.workspace.clone();
    let preflight = change.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
      let paths = preflight
        .files
        .iter()
        .map(|f| PathBuf::from(&f.path))
        .collect::<Vec<_>>();
      load_repository_instructions_for_paths(&workspace, &paths)?;
      for file in &preflight.files {
        workspace.validate_change_path(&file.path)?;
        workspace.check_revision(&file.path, &file.expected_revision)?;
      }
      Ok(())
    })
    .await??;
    self.fence(&cancel).await?;
    let proof = self
      .daemon
      .store
      .prepare_change(PrepareChange {
        request_id: &input.request_id,
        change_id: &input.change_id,
        expected_revision: input.expected_revision,
        preview_sha256: &input.preview_sha256,
        run_id: &self.run_id,
        owner_epoch: self.epoch,
        action_id: &input.action_id,
        arguments: &exact,
        approval_id: &approval,
      })
      .await?;
    self.daemon.events.notify_waiters();
    let receipt = dispatch(&self.daemon, self.workspace.clone(), proof, cancel, write).await?;
    Ok(serde_json::to_value(agent_changes::model_change(
      &receipt,
      &[],
      false,
    ))?)
  }
}

pub(super) async fn dispatch(
  daemon: &crate::Daemon,
  workspace: agent_workspace::Workspace,
  proof: agent_storage::changes::PreparedChange,
  cancel: CancellationToken,
  write: tokio::sync::OwnedMutexGuard<()>,
) -> Result<agent_changes::ChangeSet> {
  let files = proof.change().files.clone();
  // The owned guard remains with native I/O if the awaiting future is dropped.
  let effect = tokio::task::spawn_blocking(move || {
    let _write = write;
    let mut written = Vec::new();
    for file in files {
      if cancel.is_cancelled() {
        return (written, false);
      }
      let result = workspace
        .validate_change_path(&file.path)
        .and_then(|()| match file.after {
          Some(after) => workspace
            .write_change_revision(&file.path, &after, &file.expected_revision)
            .map(|_| ()),
          None => workspace.remove_revision(&file.path, &file.expected_revision),
        });
      if result.is_err() {
        return (written, false);
      }
      written.push(file.path);
    }
    (written, true)
  })
  .await;
  let (written, complete) = effect.unwrap_or_default();
  let receipt = daemon
    .store
    .finish_change(proof, written, complete)
    .await
    .map_err(|_| {
      RuntimeError::Uncertain(
        "change receipt could not be persisted; inspect checkpoint and files before further action"
          .into(),
      )
    })?;
  daemon.events.notify_waiters();
  if !complete {
    return Err(RuntimeError::Uncertain(format!("change {} has partial or unknown effects; inspect checkpoint and current revisions without replay",receipt.id)).into());
  }
  Ok(receipt)
}
