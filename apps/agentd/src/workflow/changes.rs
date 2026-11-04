use super::{binding, convert, error, session};
use crate::Daemon;
use agent_changes::ChangeSet;
use agent_protocol::v1;
use tonic::Status;
async fn output(daemon: &Daemon, change: ChangeSet) -> Result<v1::WorkflowChange, Status> {
  let validations = daemon
    .store
    .change_validations(&change.id)
    .await
    .map_err(error)?;
  Ok(convert::change(change, validations))
}
pub(super) async fn list(
  daemon: &Daemon,
  input: v1::ListWorkflowChangesRequest,
) -> Result<v1::WorkflowChanges, Status> {
  session(daemon, &input.session_id).await?;
  let limit = input.limit.clamp(1, 100) as usize;
  let after = if input.after_id.is_empty() {
    None
  } else {
    convert::id(&input.after_id)?;
    Some(input.after_id.as_str())
  };
  let mut changes = daemon
    .store
    .session_changes(&input.session_id, after, limit as u32 + 1)
    .await
    .map_err(error)?;
  let has_more = changes.len() > limit;
  changes.truncate(limit);
  let next_after = if has_more {
    changes
      .last()
      .map(|change| change.id.clone())
      .unwrap_or_default()
  } else {
    String::new()
  };
  let mut items = Vec::with_capacity(changes.len());
  for change in changes {
    let mut summary = convert::change(change, Vec::new());
    summary.detail_included = false;
    for file in &mut summary.files {
      file.before = None;
      file.after = None;
      file.unified_diff.clear();
    }
    items.push(summary);
  }
  Ok(v1::WorkflowChanges {
    changes: items,
    next_after,
  })
}
pub(super) async fn get(
  daemon: &Daemon,
  input: v1::GetWorkflowChangeRequest,
) -> Result<v1::WorkflowChange, Status> {
  session(daemon, &input.session_id).await?;
  convert::id(&input.change_id)?;
  let change = daemon
    .store
    .change_set(&input.change_id)
    .await
    .map_err(error)?;
  binding(daemon, &input.session_id, &change.run_id).await?;
  output(daemon, change).await
}
pub(super) async fn undo(
  daemon: &Daemon,
  input: v1::PreviewWorkflowUndoRequest,
) -> Result<v1::WorkflowChange, Status> {
  let current = session(daemon, &input.session_id).await?;
  if current.owner_epoch != input.owner_epoch {
    return Err(Status::aborted("session owner epoch changed"));
  }
  convert::id(&input.request_id)?;
  convert::id(&input.change_id)?;
  let source = daemon
    .store
    .change_set(&input.change_id)
    .await
    .map_err(error)?;
  binding(daemon, &input.session_id, &source.run_id).await?;
  if input.run_id != source.run_id {
    return Err(Status::invalid_argument("source checkpoint run mismatch"));
  }
  let change = daemon
    .user_preview_undo(
      &input.session_id,
      input.owner_epoch,
      &input.change_id,
      &input.request_id,
    )
    .await
    .map_err(error)?;
  daemon.events.notify_waiters();
  output(daemon, change).await
}
pub(super) async fn apply(
  daemon: &Daemon,
  input: v1::ApplyWorkflowChangeRequest,
) -> Result<v1::WorkflowChange, Status> {
  let current = session(daemon, &input.session_id).await?;
  if current.owner_epoch != input.owner_epoch {
    return Err(Status::aborted("session owner epoch changed"));
  }
  for id in [&input.request_id, &input.change_id, &input.action_id] {
    convert::id(id)?;
  }
  let stored = daemon
    .store
    .change_set(&input.change_id)
    .await
    .map_err(error)?;
  binding(daemon, &input.session_id, &stored.run_id).await?;
  if stored.run_id != input.run_id {
    return Err(Status::invalid_argument("checkpoint run mismatch"));
  }
  // This authenticated explicit user request approves only the reviewed revision and hash.
  let change = daemon
    .user_apply_change(
      &input.session_id,
      input.owner_epoch,
      &input.change_id,
      input.expected_revision,
      &input.preview_sha256,
      &input.request_id,
      &input.action_id,
    )
    .await
    .map_err(error)?;
  daemon.events.notify_waiters();
  output(daemon, change).await
}
