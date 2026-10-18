use super::*;
use agent_computer::{
  Bounds, DesktopActionRequest, DesktopSnapshot, LeaseToken, ObservationLimits, TargetScope,
};
use agent_computer_runtime::ExecutionResult;

pub(super) fn validate(
  pinned: &PinnedHost,
  expected: &Command,
  reply: &Reply,
) -> Result<(), Status> {
  let valid = match (&expected.operation, reply) {
    (Operation::Capabilities, Reply::Capabilities(capabilities)) => {
      capabilities.identity == pinned.info.desktop_session.identity
    }
    (Operation::AcquireLease { .. }, Reply::Lease(lease)) => valid_lease(pinned, lease),
    (
      Operation::RenewLease {
        lease: expected, ..
      },
      Reply::Lease(lease),
    ) => valid_lease(pinned, lease) && lease == expected,
    (Operation::Observe { lease, limits, .. }, Reply::Snapshot(snapshot)) => {
      valid_snapshot(pinned, lease, *limits, snapshot)
    }
    (Operation::Prepare { request }, Reply::Prepared(prepared)) => {
      !prepared.preparation_id.is_nil()
        && prepared.binding == expected.binding
        && prepared.request == *request
        && valid_request(pinned, &prepared.request)
    }
    (
      Operation::Execute {
        preparation_id,
        action_id,
        content_digest,
        approval_id,
        ..
      },
      Reply::Executed(executed),
    ) => {
      valid_execution(pinned, expected, *action_id, executed)
        && executed.preparation_id == *preparation_id
        && executed.content_digest == *content_digest
        && executed.receipt.approval_id == *approval_id
    }
    (Operation::Revoke { lease }, Reply::Revoked) => valid_lease(pinned, lease),
    (Operation::Receipt { .. }, Reply::Receipt(None)) => true,
    (Operation::Receipt { action_id }, Reply::Receipt(Some(executed))) => {
      valid_execution(pinned, expected, *action_id, executed)
    }
    _ => false,
  };
  if valid {
    Ok(())
  } else {
    Err(Status::invalid_argument(
      "invalid native reply binding or bounds",
    ))
  }
}

fn valid_lease(pinned: &PinnedHost, lease: &LeaseToken) -> bool {
  !lease.id.is_nil()
    && !lease.session_id.is_nil()
    && lease.session_id == pinned.info.desktop_session.id
    && lease.identity == pinned.info.desktop_session.identity
}

fn valid_request(pinned: &PinnedHost, request: &DesktopActionRequest) -> bool {
  !request.action_id.is_nil()
    && !request.snapshot_id.is_nil()
    && request.snapshot_revision > 0
    && valid_lease(pinned, &request.lease)
    && request.action.validate().is_ok()
}

fn valid_execution(
  pinned: &PinnedHost,
  expected: &Command,
  action_id: Uuid,
  executed: &ExecutionResult,
) -> bool {
  executed.binding == expected.binding
    && !executed.preparation_id.is_nil()
    && valid_request(pinned, &executed.original_request)
    && executed.original_request.action_id == action_id
    && executed.receipt.action_id == action_id
    && !executed.receipt.approval_id.is_nil()
    && !executed.receipt.snapshot_id.is_nil()
    && executed.receipt.snapshot_id != executed.original_request.snapshot_id
}

fn valid_snapshot(
  pinned: &PinnedHost,
  lease: &LeaseToken,
  limits: ObservationLimits,
  snapshot: &DesktopSnapshot,
) -> bool {
  let observed = &snapshot.observation;
  let image = &observed.screenshot;
  if !valid_lease(pinned, lease)
    || limits.validate().is_err()
    || snapshot.id.is_nil()
    || snapshot.revision == 0
    || snapshot.session != pinned.info.desktop_session
    || snapshot.lease_id != lease.id
    || limits.check_image(image.width, image.height).is_err()
    || image.rgba.len() as u64 != u64::from(image.width) * u64::from(image.height) * 4
    || !valid_bounds(observed.bounds)
    || observed.windows.len() > limits.max_windows
    || observed.accessibility.nodes.len() > limits.max_nodes
  {
    return false;
  }
  if observed
    .windows
    .iter()
    .any(|window| !valid_bounds(window.bounds) || !intersects(observed.bounds, window.bounds))
  {
    return false;
  }
  if let TargetScope::Window(id) = snapshot.session.scope
    && (observed.windows.len() != 1
      || observed.windows[0].id != id
      || observed.windows[0].bounds != observed.bounds)
  {
    return false;
  }
  let mut depths = Vec::with_capacity(observed.accessibility.nodes.len());
  for (index, node) in observed.accessibility.nodes.iter().enumerate() {
    if node.index != index {
      return false;
    }
    let depth = match node.parent {
      Some(parent) if parent < index => depths[parent] + 1,
      Some(_) => return false,
      None => 0,
    };
    if depth > limits.max_depth {
      return false;
    }
    depths.push(depth);
  }
  true
}

fn valid_bounds(bounds: Bounds) -> bool {
  bounds.width > 0 && bounds.height > 0
}

fn intersects(a: Bounds, b: Bounds) -> bool {
  i64::from(a.x) < i64::from(b.x) + i64::from(b.width)
    && i64::from(b.x) < i64::from(a.x) + i64::from(a.width)
    && i64::from(a.y) < i64::from(b.y) + i64::from(b.height)
    && i64::from(b.y) < i64::from(a.y) + i64::from(a.height)
}
