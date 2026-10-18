use agent_core::{Session, Task};
use agent_protocol::v1;
use tonic::Status;

use super::failure;
use crate::Daemon;

pub(super) fn identifier(value: &str) -> Result<(), Status> {
  if value.is_empty()
    || value.len() > 256
    || !value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
  {
    return Err(Status::invalid_argument(
      "invalid task, session, run, or node identifier",
    ));
  }
  Ok(())
}

pub(super) fn target(value: Option<v1::Node>) -> Result<v1::Node, Status> {
  let value = value.ok_or_else(|| Status::invalid_argument("target node is required"))?;
  identifier(&value.id)?;
  if value.endpoint.len() > 2048 {
    return Err(Status::invalid_argument("invalid peer endpoint"));
  }
  let endpoint = url::Url::parse(&value.endpoint)
    .map_err(|_| Status::invalid_argument("invalid peer endpoint"))?;
  if endpoint.scheme() != "https"
    || endpoint.host_str().is_none()
    || !endpoint.username().is_empty()
    || endpoint.password().is_some()
    || endpoint.query().is_some()
    || endpoint.fragment().is_some()
    || endpoint.path() != "/"
  {
    return Err(Status::invalid_argument(
      "peer endpoint must be a credential-free HTTPS origin",
    ));
  }
  // Only the durable pair record authorizes trust. Caller-provided fingerprints,
  // capabilities and protocol claims cannot alter the authenticated handshake.
  Ok(v1::Node {
    id: value.id,
    endpoint: endpoint.to_string(),
    ..Default::default()
  })
}

impl Daemon {
  pub(super) fn master_session(&self, id: &str) -> Result<Session, Status> {
    identifier(id)?;
    let session = self
      .store
      .session(id)
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id || id.starts_with("worker:") {
      return Err(Status::permission_denied(
        "session is not owned by this Master",
      ));
    }
    Ok(session)
  }

  pub(super) fn master_task(&self, id: &str) -> Result<Task, Status> {
    identifier(id)?;
    let task = self
      .store
      .task(id)
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("task not found"))?;
    let session = self.master_session(&task.envelope.session_id)?;
    if task.envelope.owner_node_id != self.node_id
      || task.envelope.owner_epoch != session.owner_epoch
      || self.store.task_target(id).map_err(failure)?.is_none()
    {
      return Err(Status::permission_denied(
        "task is not owned by the current Master epoch",
      ));
    }
    Ok(task)
  }

  pub(super) fn master_task_target(&self, id: &str, target: &v1::Node) -> Result<(), Status> {
    self.master_task(id)?;
    if self.store.task_target(id).map_err(failure)?.as_deref() != Some(target.id.as_str()) {
      return Err(Status::permission_denied(
        "task is assigned to another Worker",
      ));
    }
    Ok(())
  }

  pub(super) fn validate_dispatch(
    &self,
    target: &v1::Node,
    envelope: &v1::TaskEnvelope,
  ) -> Result<(), Status> {
    for id in [
      &envelope.task_id,
      &envelope.attempt_id,
      &envelope.parent_run_id,
      &envelope.session_id,
      &envelope.owner_node_id,
      &envelope.idempotency_key,
    ] {
      identifier(id)?;
    }
    if uuid::Uuid::parse_str(&envelope.task_id).is_err()
      || uuid::Uuid::parse_str(&envelope.attempt_id).is_err()
    {
      return Err(Status::invalid_argument(
        "task and attempt identifiers must be UUIDs",
      ));
    }
    let payload_size = envelope
      .instructions
      .len()
      .saturating_add(envelope.workspace.len())
      .saturating_add(envelope.task_id.len())
      .saturating_add(envelope.attempt_id.len())
      .saturating_add(envelope.parent_run_id.len())
      .saturating_add(envelope.session_id.len())
      .saturating_add(envelope.owner_node_id.len())
      .saturating_add(envelope.idempotency_key.len())
      .saturating_add(envelope.capabilities.iter().map(String::len).sum::<usize>());
    if envelope.instructions.trim().is_empty() || payload_size > 64 * 1024 {
      return Err(Status::invalid_argument(
        "task payload must be nonempty and at most 64 KiB",
      ));
    }
    let remaining = envelope.deadline_ms.saturating_sub(agent_core::now_ms());
    if !(1..=86_400_000).contains(&remaining) {
      return Err(Status::invalid_argument(
        "task deadline must be within the next 24 hours",
      ));
    }
    let session = self.master_session(&envelope.session_id)?;
    if envelope.owner_node_id != self.node_id || envelope.owner_epoch != session.owner_epoch {
      return Err(Status::permission_denied(
        "task owner or epoch differs from the current Master",
      ));
    }
    if envelope.workspace != session.workspace {
      return Err(Status::invalid_argument(
        "task workspace differs from its session",
      ));
    }
    let run = self
      .store
      .run(&envelope.parent_run_id)
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("parent run not found"))?;
    if run.session_id != session.id || run.status != "running" {
      return Err(Status::failed_precondition(
        "parent run must be active in the task session",
      ));
    }
    if self
      .store
      .task(&envelope.task_id)
      .map_err(failure)?
      .is_some()
    {
      return Err(Status::already_exists(
        "task identity already exists; reconcile its current state",
      ));
    }
    if envelope.capabilities.len() > 2
      || envelope
        .capabilities
        .iter()
        .any(|cap| !matches!(cap.as_str(), "workspace.read" | "workspace.write.approval"))
    {
      return Err(Status::permission_denied(
        "task capability scope is not supported",
      ));
    }
    let trust = crate::peers::manager(self)
      .map_err(failure)?
      .trust(&target.id)
      .map_err(|_| Status::permission_denied("target is not a paired active peer"))?;
    if !envelope
      .capabilities
      .iter()
      .all(|cap| trust.capabilities.contains(cap))
    {
      return Err(Status::permission_denied(
        "task capabilities exceed the paired peer scope",
      ));
    }
    Ok(())
  }
}
