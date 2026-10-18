use std::time::Duration;

use uuid::Uuid;

use crate::{
  content::content_digest,
  runtime::{CapturedSnapshot, RuntimeState},
  *,
};

/// Immutable in-process approval binding. This is neither wire data nor approval:
/// the host must authorize its request and persist intent before execution.
/// Its private runtime identity prevents reuse after helper/runtime replacement.
pub struct PreparedDesktopAction {
  instance: Uuid,
  request: DesktopActionRequest,
  digest: [u8; 32],
  limits: ObservationLimits,
}

impl PreparedDesktopAction {
  pub fn request(&self) -> &DesktopActionRequest {
    &self.request
  }
  pub fn content_digest(&self) -> &[u8; 32] {
    &self.digest
  }
}

impl DesktopRuntime {
  /// Bind the exact fresh snapshot, lease and action before requesting approval.
  pub fn prepare_action(
    &self,
    request: DesktopActionRequest,
  ) -> Result<PreparedDesktopAction, DesktopError> {
    request.action.validate()?;
    self.check_session(&request.lease)?;
    self.leases.validate(&request.lease)?;
    let state = self
      .state
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    if state.receipts.contains_key(&request.action_id) {
      return Err(DesktopError::DuplicateAction);
    }
    let captured = self.current_snapshot(&state, &request)?;
    let digest = content_digest(&captured.snapshot.session, &captured.snapshot.observation)?;
    self.leases.validate(&request.lease)?;
    self.current_snapshot(&state, &request)?;
    Ok(PreparedDesktopAction {
      instance: self.instance,
      request,
      digest,
      limits: captured.limits,
    })
  }

  /// Refresh after delayed approval using the original lease and capture bounds.
  /// Only snapshot ID/revision may change; content must remain exactly identical.
  /// This does not dispatch input or create an authorization attestation.
  pub fn refresh_prepared_action(
    &self,
    prepared: &PreparedDesktopAction,
    control: OperationControl,
  ) -> Result<DesktopActionRequest, DesktopError> {
    if self.instance != prepared.instance {
      return Err(DesktopError::StaleSnapshot);
    }
    self.check_session(&prepared.request.lease)?;
    self.leases.validate(&prepared.request.lease)?;
    {
      let state = self
        .state
        .lock()
        .map_err(|_| DesktopError::CoordinationUnavailable)?;
      if state.receipts.contains_key(&prepared.request.action_id) {
        return Err(DesktopError::DuplicateAction);
      }
    }
    let snapshot = self.observe(&prepared.request.lease, prepared.limits, control.clone())?;
    if content_digest(&snapshot.session, &snapshot.observation)? != prepared.digest {
      return Err(DesktopError::StaleSnapshot);
    }
    control.checkpoint(self.leases.now())?;
    self.leases.validate(&prepared.request.lease)?;
    let request = DesktopActionRequest {
      snapshot_id: snapshot.id,
      snapshot_revision: snapshot.revision,
      ..prepared.request.clone()
    };
    let state = self
      .state
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    if state.receipts.contains_key(&request.action_id) {
      return Err(DesktopError::DuplicateAction);
    }
    self.current_snapshot(&state, &request)?;
    Ok(request)
  }

  pub(crate) fn current_snapshot<'a>(
    &self,
    state: &'a RuntimeState,
    request: &DesktopActionRequest,
  ) -> Result<&'a CapturedSnapshot, DesktopError> {
    let captured = state.snapshot.as_ref().ok_or(DesktopError::StaleSnapshot)?;
    let snapshot = &captured.snapshot;
    if snapshot.id != request.snapshot_id
      || snapshot.revision != request.snapshot_revision
      || snapshot.lease_id != request.lease.id
      || snapshot.session != self.session
      || self
        .leases
        .now()
        .saturating_duration_since(captured.captured)
        >= Duration::from_secs(2)
    {
      return Err(DesktopError::StaleSnapshot);
    }
    if let Some((x, y)) = request.action.point()
      && !snapshot.observation.bounds.contains(x, y)
    {
      return Err(DesktopError::InvalidRequest(
        "point outside authorized scope".into(),
      ));
    }
    Ok(captured)
  }
}
