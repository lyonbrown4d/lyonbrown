use agent_app::RuntimeSnapshot;

use super::Runtime;
use crate::tui::{ApprovalRequest, RemoteProjection, apply_remote_state};

pub fn sync_runtime_snapshot(runtime: &mut Runtime) {
  if runtime.remote_state.is_some() {
    sync_remote(runtime);
    return;
  }
  let changed = !runtime.state.has_observed_snapshot() || runtime.snapshots.borrow().has_changed();
  if !changed {
    return;
  }
  let snapshot = runtime.snapshots.borrow_and_update();
  sync_approval(
    &mut runtime.state,
    &mut runtime.approval,
    &mut runtime.approval_scroll,
    Some(&snapshot),
  );
  runtime.state.sync_snapshot(&snapshot);
}

fn sync_remote(runtime: &mut Runtime) {
  let changed = !runtime.state.has_synced_remote_runtime()
    || runtime
      .remote_state
      .as_ref()
      .is_some_and(|state| state.borrow().has_changed());
  if !changed {
    return;
  }
  let remote = runtime
    .remote_state
    .as_mut()
    .expect("remote receiver was checked")
    .borrow_and_update();
  let projection = apply_remote_state(&mut runtime.state, &remote);
  if projection == RemoteProjection::Accepted {
    sync_approval(
      &mut runtime.state,
      &mut runtime.approval,
      &mut runtime.approval_scroll,
      remote.snapshot.as_ref().map(|snapshot| &snapshot.snapshot),
    );
  }
  runtime.state.mark_remote_runtime_synced();
}

fn sync_approval(
  state: &mut crate::tui::TuiState,
  approval: &mut Option<ApprovalRequest>,
  approval_scroll: &mut u16,
  snapshot: Option<&RuntimeSnapshot>,
) {
  let next_approval = snapshot
    .and_then(|snapshot| snapshot.approval.as_ref())
    .and_then(ApprovalRequest::from_view);
  let approval_changed = approval.as_ref().map(|request| request.request_id)
    != next_approval.as_ref().map(|request| request.request_id);
  if approval_changed {
    *approval_scroll = 0;
    if next_approval.is_some() {
      state.close_overlay();
    }
  }
  *approval = next_approval;
  state.set_queued_follow_up(
    snapshot.and_then(|snapshot| snapshot.queued_follow_ups.first().cloned()),
  );
}
