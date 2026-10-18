use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use uuid::Uuid;

use crate::*;

pub(crate) struct RuntimeState {
  revision: u64,
  pub(crate) snapshot: Option<CapturedSnapshot>,
  pub(crate) receipts: HashMap<Uuid, DesktopActionReceipt>,
}

pub(crate) struct CapturedSnapshot {
  pub(crate) snapshot: DesktopSnapshot,
  pub(crate) captured: Instant,
  pub(crate) limits: ObservationLimits,
}

/// Synchronous native boundary: the host should run calls on its bounded blocking
/// executor. Use an authenticated helper in the interactive user session.
pub struct DesktopRuntime {
  pub(crate) instance: Uuid,
  pub(crate) session: DesktopSession,
  pub(crate) leases: Arc<LeaseRegistry>,
  backend: Mutex<Box<dyn DesktopBackend>>,
  pub(crate) state: Mutex<RuntimeState>,
}

impl DesktopRuntime {
  pub fn new(
    session: DesktopSession,
    leases: Arc<LeaseRegistry>,
    backend: Box<dyn DesktopBackend>,
  ) -> Self {
    Self {
      instance: Uuid::new_v4(),
      session,
      leases,
      backend: Mutex::new(backend),
      state: Mutex::new(RuntimeState {
        revision: 0,
        snapshot: None,
        receipts: HashMap::new(),
      }),
    }
  }

  pub fn capabilities(&self) -> Result<DesktopCapabilities, DesktopError> {
    let capabilities = self
      .backend
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?
      .capabilities()?;
    if capabilities.identity != self.session.identity {
      return Err(DesktopError::SessionUnavailable);
    }
    Ok(capabilities)
  }

  pub fn acquire_lease(&self, ttl: Duration) -> Result<LeaseToken, DesktopError> {
    let capabilities = self.capabilities()?;
    if !capabilities.interactive {
      return Err(DesktopError::SessionUnavailable);
    }
    self.leases.acquire(&self.session, ttl)
  }

  pub fn renew_lease(&self, lease: &LeaseToken, ttl: Duration) -> Result<(), DesktopError> {
    self.check_session(lease)?;
    if !self.capabilities()?.can_control() {
      return Err(DesktopError::PermissionDenied("desktop control".into()));
    }
    self.leases.renew(lease, ttl)
  }

  pub fn revoke(&self) -> Result<(), DesktopError> {
    self.leases.revoke(&self.session.identity)
  }

  pub fn observe(
    &self,
    lease: &LeaseToken,
    limits: ObservationLimits,
    control: OperationControl,
  ) -> Result<DesktopSnapshot, DesktopError> {
    limits.validate()?;
    self.check_session(lease)?;
    let guard = ExecutionGuard {
      lease: self.leases.begin(lease)?,
      control,
    };
    guard.checkpoint()?;
    let mut backend = self
      .backend
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    guard.checkpoint()?;
    let observed_at = self.leases.now();
    let observation = backend.observe(&self.session, limits, &guard.control)?;
    guard.checkpoint()?;
    if self.leases.now().saturating_duration_since(observed_at) >= Duration::from_secs(2) {
      return Err(DesktopError::StaleSnapshot);
    }
    limits.check_image(observation.screenshot.width, observation.screenshot.height)?;
    let expected =
      u64::from(observation.screenshot.width) * u64::from(observation.screenshot.height) * 4;
    if observation.screenshot.rgba.len() as u64 != expected
      || observation.windows.len() > limits.max_windows
      || observation.accessibility.nodes.len() > limits.max_nodes
    {
      return Err(DesktopError::CapacityExceeded);
    }
    let mut state = self
      .state
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    state.revision = state
      .revision
      .checked_add(1)
      .ok_or(DesktopError::CapacityExceeded)?;
    let snapshot = DesktopSnapshot {
      id: Uuid::new_v4(),
      revision: state.revision,
      lease_id: lease.id,
      session: self.session.clone(),
      observation,
    };
    state.snapshot = Some(CapturedSnapshot {
      snapshot: snapshot.clone(),
      captured: observed_at,
      limits,
    });
    Ok(snapshot)
  }

  /// The host must persist intent BEFORE constructing `authorized`, then persist
  /// the returned receipt. Restarts must restore durable action IDs at the host;
  /// this in-memory guard only prevents replay within this runtime's lifetime.
  pub fn execute(
    &self,
    authorized: AuthorizedAction,
    control: OperationControl,
  ) -> Result<DesktopActionReceipt, DesktopError> {
    let request = authorized.request;
    request.action.validate()?;
    self.check_session(&request.lease)?;
    let guard = ExecutionGuard {
      lease: self.leases.begin(&request.lease)?,
      control,
    };
    guard.checkpoint()?;
    let mut backend = self
      .backend
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    guard.checkpoint()?;
    let snapshot = {
      let mut state = self
        .state
        .lock()
        .map_err(|_| DesktopError::CoordinationUnavailable)?;
      if state.receipts.contains_key(&request.action_id) {
        return Err(DesktopError::DuplicateAction);
      }
      if state.receipts.len() >= 4096 {
        return Err(DesktopError::CapacityExceeded);
      }
      let captured = self.current_snapshot(&state, &request)?;
      let snapshot = &captured.snapshot;
      let snapshot = snapshot.clone();
      // Consume before entering the native boundary, including rejected attempts.
      state.snapshot = None;
      state.receipts.insert(
        request.action_id,
        DesktopActionReceipt {
          action_id: request.action_id,
          approval_id: authorized.approval_id,
          snapshot_id: request.snapshot_id,
          outcome: ActionOutcome::Uncertain(DesktopError::BackendUnavailable(
            "execution did not return".into(),
          )),
        },
      );
      snapshot
    };
    let outcome = match backend
      .validate_snapshot(&snapshot)
      .and_then(|()| guard.checkpoint())
    {
      Ok(()) => {
        let outcome = backend.execute(&request.action, &snapshot, &guard);
        match guard.checkpoint() {
          Err(error) if !matches!(outcome, ActionOutcome::Rejected(_)) => {
            ActionOutcome::Uncertain(error)
          }
          _ => outcome,
        }
      }
      Err(error) => ActionOutcome::Rejected(error),
    };
    let receipt = DesktopActionReceipt {
      action_id: request.action_id,
      approval_id: authorized.approval_id,
      snapshot_id: request.snapshot_id,
      outcome,
    };
    self
      .state
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?
      .receipts
      .insert(request.action_id, receipt.clone());
    Ok(receipt)
  }

  pub fn receipt(&self, action_id: Uuid) -> Result<Option<DesktopActionReceipt>, DesktopError> {
    Ok(
      self
        .state
        .lock()
        .map_err(|_| DesktopError::CoordinationUnavailable)?
        .receipts
        .get(&action_id)
        .cloned(),
    )
  }

  pub(crate) fn check_session(&self, token: &LeaseToken) -> Result<(), DesktopError> {
    if token.session_id != self.session.id || token.identity != self.session.identity {
      return Err(DesktopError::LeaseInvalid);
    }
    Ok(())
  }
}
