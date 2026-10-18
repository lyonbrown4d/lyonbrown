#![allow(dead_code)]

use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
  },
  time::{Duration, Instant},
};

use agent_computer::*;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Clock {
  start: Instant,
  millis: AtomicU64,
}

impl Clock {
  pub fn new() -> Self {
    Self {
      start: Instant::now(),
      millis: AtomicU64::new(0),
    }
  }
  pub fn advance(&self, milliseconds: u64) {
    self.millis.fetch_add(milliseconds, Ordering::SeqCst);
  }
}

impl DesktopClock for Clock {
  fn now(&self) -> Instant {
    self.start + Duration::from_millis(self.millis.load(Ordering::SeqCst))
  }
}

#[derive(Default)]
pub struct Effects {
  pub calls: usize,
  pub observation: Option<DesktopObservation>,
  pub observed_limits: Vec<ObservationLimits>,
  pub validate_error: Option<DesktopError>,
  pub outcome: Option<ActionOutcome>,
  pub cancel_during_input: Option<CancellationToken>,
  pub invalid_image: bool,
  pub advance_during_capture: Option<(Arc<Clock>, u64)>,
}

pub struct Backend {
  pub session: DesktopSession,
  pub effects: Arc<Mutex<Effects>>,
}

impl DesktopBackend for Backend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    Ok(DesktopCapabilities {
      identity: self.session.identity.clone(),
      interactive: true,
      capture: PermissionState::Granted,
      input: PermissionState::Granted,
      accessibility: PermissionState::Unsupported,
    })
  }
  fn observe(
    &mut self,
    _: &DesktopSession,
    limits: ObservationLimits,
    _: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    {
      let mut effects = self.effects.lock().expect("effects");
      effects.observed_limits.push(limits);
      if let Some(observation) = &effects.observation {
        return Ok(observation.clone());
      }
    }
    let invalid_image = self.effects.lock().expect("effects").invalid_image;
    if let Some((clock, milliseconds)) =
      &self.effects.lock().expect("effects").advance_during_capture
    {
      clock.advance(*milliseconds);
    }
    Ok(DesktopObservation {
      bounds: Bounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      screenshot: Screenshot {
        width: 2,
        height: 2,
        rgba: vec![0; if invalid_image { 3 } else { 16 }],
      },
      windows: vec![],
      windows_truncated: false,
      accessibility: AccessibilityObservation {
        state: PermissionState::Unsupported,
        nodes: vec![],
        truncated: false,
      },
    })
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    match self.effects.lock().expect("effects").validate_error.clone() {
      Some(error) => Err(error),
      None => Ok(()),
    }
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    if let Err(error) = guard.checkpoint() {
      return ActionOutcome::Rejected(error);
    }
    let mut effects = self.effects.lock().expect("effects");
    effects.calls += 1;
    if let Some(token) = &effects.cancel_during_input {
      token.cancel();
    }
    effects.outcome.clone().unwrap_or(ActionOutcome::Completed)
  }
}

pub struct Fixture {
  pub clock: Arc<Clock>,
  pub leases: Arc<LeaseRegistry>,
  pub runtime: DesktopRuntime,
  pub effects: Arc<Mutex<Effects>>,
  pub session: DesktopSession,
}

impl Fixture {
  pub fn new() -> Self {
    let clock = Arc::new(Clock::new());
    let leases = Arc::new(LeaseRegistry::new(clock.clone()));
    let session = session();
    let effects = Arc::new(Mutex::new(Effects::default()));
    let backend = Backend {
      session: session.clone(),
      effects: effects.clone(),
    };
    let runtime = DesktopRuntime::new(session.clone(), leases.clone(), Box::new(backend));
    Self {
      clock,
      leases,
      runtime,
      effects,
      session,
    }
  }
  pub fn control(&self) -> OperationControl {
    OperationControl {
      cancellation: CancellationToken::new(),
      deadline: self.clock.now() + Duration::from_secs(10),
    }
  }
  pub fn lease(&self) -> LeaseToken {
    self
      .runtime
      .acquire_lease(Duration::from_secs(30))
      .expect("lease")
  }
  pub fn snapshot(&self, lease: &LeaseToken) -> DesktopSnapshot {
    self
      .runtime
      .observe(lease, ObservationLimits::default(), self.control())
      .expect("observation")
  }
}

pub fn session() -> DesktopSession {
  DesktopSession {
    id: Uuid::new_v4(),
    identity: DesktopIdentity {
      node_id: "fixture-node".into(),
      user_session: "fixture-user-desktop".into(),
    },
    scope: TargetScope::Window(42),
  }
}

pub fn request(lease: &LeaseToken, snapshot: &DesktopSnapshot) -> DesktopActionRequest {
  DesktopActionRequest {
    action_id: Uuid::new_v4(),
    lease: lease.clone(),
    snapshot_id: snapshot.id,
    snapshot_revision: snapshot.revision,
    action: DesktopAction::Click {
      x: 1,
      y: 1,
      button: MouseButton::Left,
    },
  }
}

pub fn approved(request: DesktopActionRequest) -> AuthorizedAction {
  AuthorizedAction::new(request, Uuid::new_v4())
}
