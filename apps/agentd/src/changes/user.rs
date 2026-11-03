//! Human-only workflow commands. Runtime tools never call this entry point.
use crate::Daemon;
use agent_changes::{ChangeSet, ChangeState, reverse_preview};
use agent_storage::changes::UserPrepareChange;
use agent_workspace::{Workspace, load_repository_instructions_for_paths};
use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

impl Daemon {
  async fn user_change_session(&self, id: &str, epoch: u64) -> Result<agent_core::Session> {
    ensure!(!self.stopping.is_cancelled(), "daemon is stopping");
    let session = self
      .store
      .session(id)
      .await?
      .context("user change session missing")?;
    ensure!(
      session.owner_node_id == self.node_id && session.owner_epoch == epoch,
      "user change ownership changed"
    );
    Ok(session)
  }
  pub(crate) async fn user_preview_undo(
    &self,
    session_id: &str,
    owner_epoch: u64,
    source_change_id: &str,
    request_id: &str,
  ) -> Result<ChangeSet> {
    let _submission = self.submission.lock().await;
    let session = self.user_change_session(session_id, owner_epoch).await?;
    let source = self.store.change_set(source_change_id).await?;
    let run = self
      .store
      .run(&source.run_id)
      .await?
      .context("checkpoint audit run missing")?;
    ensure!(
      run.session_id == session.id,
      "checkpoint belongs to another session"
    );
    let change = reverse_preview(
      request_id.into(),
      source.run_id.clone(),
      owner_epoch,
      &source,
    )?;
    let workspace = Workspace::open(&session.workspace)?;
    let checked = change.clone();
    tokio::task::spawn_blocking(move || check(&workspace, &checked)).await??;
    let saved = self
      .store
      .save_user_change_preview(session_id, owner_epoch, request_id, change)
      .await?;
    self.events.notify_waiters();
    Ok(saved)
  }
  #[allow(clippy::too_many_arguments)]
  pub(crate) async fn user_apply_change(
    &self,
    session_id: &str,
    owner_epoch: u64,
    change_id: &str,
    expected_revision: u64,
    preview_sha256: &str,
    request_id: &str,
    action_id: &str,
  ) -> Result<ChangeSet> {
    // Also exclude new model submission until the filesystem receipt is saved.
    let _submission = self.submission.lock().await;
    let session = self.user_change_session(session_id, owner_epoch).await?;
    let change = self.store.change_set(change_id).await?;
    ensure!(
      change.state == ChangeState::Preview
        && change.revision == expected_revision
        && change.preview_sha256 == preview_sha256,
      "reviewed preview mismatch"
    );
    let workspace = Workspace::open(&session.workspace)?;
    let write = self.workspace_writes.clone().lock_owned().await;
    let checked = change.clone();
    let preflight = workspace.clone();
    tokio::task::spawn_blocking(move || check(&preflight, &checked)).await??;
    self.user_change_session(session_id, owner_epoch).await?;
    let proof = self
      .store
      .prepare_user_change(UserPrepareChange {
        session_id,
        owner_epoch,
        request_id,
        change_id,
        expected_revision,
        preview_sha256,
        action_id,
      })
      .await?;
    self.events.notify_waiters();
    super::apply::dispatch(self, workspace, proof, self.stopping.child_token(), write).await
  }
}
fn check(workspace: &Workspace, change: &ChangeSet) -> Result<()> {
  let paths = change
    .files
    .iter()
    .map(|file| PathBuf::from(&file.path))
    .collect::<Vec<_>>();
  load_repository_instructions_for_paths(workspace, &paths)?;
  for file in &change.files {
    workspace.validate_change_path(&file.path)?;
    workspace.check_revision(&file.path, &file.expected_revision)?;
  }
  Ok(())
}
