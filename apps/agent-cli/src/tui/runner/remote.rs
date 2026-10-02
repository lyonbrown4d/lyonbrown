use agent_app::RuntimeSnapshot;

use super::Runtime;
use crate::tui::{ApprovalRequest, apply_remote_state};

pub(super) fn sync_runtime_snapshot(runtime: &mut Runtime) {
  if let Some(remote_state) = runtime.remote_state.as_ref() {
    let remote_state = remote_state.borrow().clone();
    apply_remote_state(&mut runtime.state, &remote_state);
    let snapshot = remote_state.snapshot.map(|snapshot| snapshot.snapshot);
    sync_approval(runtime, snapshot.as_ref());
    return;
  }
  let snapshot = runtime.snapshots.borrow().clone();
  sync_approval(runtime, Some(&snapshot));
  runtime.state.sync_snapshot(&snapshot);
}

fn sync_approval(runtime: &mut Runtime, snapshot: Option<&RuntimeSnapshot>) {
  let next_approval = snapshot
    .and_then(|snapshot| snapshot.approval.as_ref())
    .and_then(ApprovalRequest::from_view);
  if runtime.approval.as_ref().map(|request| request.request_id)
    != next_approval.as_ref().map(|request| request.request_id)
  {
    runtime.approval_scroll = 0;
  }
  runtime.approval = next_approval;
  runtime.state.set_queued_follow_up(
    snapshot.and_then(|snapshot| snapshot.queued_follow_ups.first().cloned()),
  );
}
