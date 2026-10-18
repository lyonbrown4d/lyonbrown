use std::{
  collections::HashMap,
  sync::{Arc, Mutex, MutexGuard},
  time::Duration,
};

use agent_computer::{
  DesktopBackend, DesktopClock, DesktopRuntime, DesktopSession, LeaseRegistry, LeaseToken,
  OperationControl, PreparedDesktopAction, SystemDesktopClock,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::*;

pub(crate) const MAX_IDENTITIES: usize = 4096;
pub(crate) const MAX_PENDING: usize = 64;
pub const MAX_TIMEOUT_MS: u64 = 30_000;

pub(crate) struct RetainedPreparation {
  pub description: Preparation,
  pub prepared: PreparedDesktopAction,
}

pub(crate) struct ActionRecord {
  pub binding: RunBinding,
  pub result: Option<ExecutionResult>,
}

#[derive(Default)]
pub(crate) struct State {
  pub leases: HashMap<Uuid, (RunBinding, LeaseToken)>,
  pub pending: HashMap<Uuid, RetainedPreparation>,
  // Never evict identities: even failed refreshes and revoked handles are tombstones.
  pub actions: HashMap<Uuid, ActionRecord>,
}

/// One immutable native desktop session. Shared leases provide exclusivity across
/// engines; the embedding host must authenticate callers and persist run ownership.
/// Capacity exhaustion fails closed until a new process; durable replay protection
/// across process replacement is the daemon's responsibility.
pub struct ComputerRuntime {
  pub(crate) runtime: DesktopRuntime,
  pub(crate) leases: Arc<LeaseRegistry>,
  pub(crate) clock: Arc<dyn DesktopClock>,
  pub(crate) state: Mutex<State>,
}

impl ComputerRuntime {
  pub fn new(
    session: DesktopSession,
    backend: Box<dyn DesktopBackend>,
    leases: Arc<LeaseRegistry>,
  ) -> Self {
    Self::with_clock(session, backend, leases, Arc::new(SystemDesktopClock))
  }

  /// Supply the same monotonic clock used by `LeaseRegistry`.
  pub fn with_clock(
    session: DesktopSession,
    backend: Box<dyn DesktopBackend>,
    leases: Arc<LeaseRegistry>,
    clock: Arc<dyn DesktopClock>,
  ) -> Self {
    Self {
      runtime: DesktopRuntime::new(session, leases.clone(), backend),
      leases,
      clock,
      state: Mutex::new(State::default()),
    }
  }

  pub fn handle(&self, command: Command, cancellation: CancellationToken) -> RuntimeResult<Reply> {
    command.binding.validate()?;
    if cancellation.is_cancelled() {
      return Err(agent_computer::DesktopError::Cancelled.into());
    }
    let binding = command.binding;
    match command.operation {
      Operation::Capabilities => Ok(Reply::Capabilities(self.runtime.capabilities()?)),
      Operation::AcquireLease { ttl_ms } => {
        let mut state = self.lock()?;
        if state.leases.len() >= MAX_IDENTITIES {
          return Err(RuntimeError::CapacityExceeded);
        }
        let lease = self.runtime.acquire_lease(Duration::from_millis(ttl_ms))?;
        state.leases.insert(lease.id, (binding, lease.clone()));
        Ok(Reply::Lease(lease))
      }
      Operation::Observe {
        lease,
        limits,
        timeout_ms,
      } => {
        self.owned_lease(&binding, &lease)?;
        let control = self.control(timeout_ms, cancellation)?;
        Ok(Reply::Snapshot(
          self.runtime.observe(&lease, limits, control)?,
        ))
      }
      Operation::Prepare { request } => self.prepare(binding, request),
      Operation::Execute {
        preparation_id,
        action_id,
        content_digest,
        approval_id,
        timeout_ms,
      } => self.execute(
        binding,
        preparation_id,
        action_id,
        content_digest,
        approval_id,
        self.control(timeout_ms, cancellation)?,
      ),
      Operation::Revoke { lease } => {
        let mut state = self.lock()?;
        Self::check_owner(&state, &binding, &lease)?;
        // Exact token validation and revocation share the registry lock.
        self.leases.revoke_token(&lease)?;
        state
          .pending
          .retain(|_, p| p.description.request.lease.id != lease.id);
        Ok(Reply::Revoked)
      }
      Operation::Receipt { action_id } => {
        let state = self.lock()?;
        let result = match state.actions.get(&action_id) {
          Some(record) if record.binding != binding => return Err(RuntimeError::BindingMismatch),
          Some(record) => record.result.clone(),
          None => None,
        };
        Ok(Reply::Receipt(result))
      }
    }
  }

  pub(crate) fn lock(&self) -> RuntimeResult<MutexGuard<'_, State>> {
    self
      .state
      .lock()
      .map_err(|_| RuntimeError::CoordinationUnavailable)
  }

  pub(crate) fn check_owner(
    state: &State,
    binding: &RunBinding,
    lease: &LeaseToken,
  ) -> RuntimeResult<()> {
    match state.leases.get(&lease.id) {
      Some((owner, token)) if owner == binding && token == lease => Ok(()),
      _ => Err(RuntimeError::BindingMismatch),
    }
  }

  pub(crate) fn owned_lease(&self, binding: &RunBinding, lease: &LeaseToken) -> RuntimeResult<()> {
    {
      let state = self.lock()?;
      Self::check_owner(&state, binding, lease)?;
    }
    self.leases.validate(lease)?;
    Ok(())
  }

  fn control(
    &self,
    timeout_ms: u64,
    cancellation: CancellationToken,
  ) -> RuntimeResult<OperationControl> {
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
      return Err(RuntimeError::InvalidControl);
    }
    let deadline = self
      .clock
      .now()
      .checked_add(Duration::from_millis(timeout_ms))
      .ok_or(RuntimeError::InvalidControl)?;
    Ok(OperationControl {
      cancellation,
      deadline,
    })
  }
}
