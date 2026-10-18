#![allow(dead_code)]
use agent_computer::*;
use agent_computer_runtime::*;
use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
  },
  time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Clock {
  start: Instant,
  milliseconds: AtomicU64,
}
impl Clock {
  pub fn new() -> Self {
    Self {
      start: Instant::now(),
      milliseconds: AtomicU64::new(0),
    }
  }
  pub fn advance(&self, milliseconds: u64) {
    self.milliseconds.fetch_add(milliseconds, Ordering::SeqCst);
  }
}
impl DesktopClock for Clock {
  fn now(&self) -> Instant {
    self.start + Duration::from_millis(self.milliseconds.load(Ordering::SeqCst))
  }
}
#[derive(Default)]
pub struct Effects {
  pub input: usize,
  pub pixel: u8,
  pub limits: Vec<ObservationLimits>,
  pub cancel_input: Option<CancellationToken>,
}
struct Backend {
  session: DesktopSession,
  effects: Arc<Mutex<Effects>>,
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
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    assert!(!control.cancellation.is_cancelled());
    let mut effects = self.effects.lock().expect("effects");
    effects.limits.push(limits);
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
        rgba: vec![effects.pixel; 16],
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
  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    if snapshot.observation.screenshot.rgba[0] != self.effects.lock().expect("effects").pixel {
      return Err(DesktopError::StaleSnapshot);
    }
    Ok(())
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
    effects.input += 1;
    if let Some(cancellation) = &effects.cancel_input {
      cancellation.cancel();
    }
    ActionOutcome::Completed
  }
}
pub struct Fixture {
  pub engine: ComputerRuntime,
  pub clock: Arc<Clock>,
  pub effects: Arc<Mutex<Effects>>,
  pub binding: RunBinding,
}
impl Fixture {
  pub fn new() -> Self {
    let clock = Arc::new(Clock::new());
    let effects = Arc::new(Mutex::new(Effects::default()));
    let session = DesktopSession {
      id: Uuid::new_v4(),
      identity: DesktopIdentity {
        node_id: "test".into(),
        user_session: "test".into(),
      },
      scope: TargetScope::Window(1),
    };
    let engine = ComputerRuntime::with_clock(
      session.clone(),
      Box::new(Backend {
        session,
        effects: effects.clone(),
      }),
      Arc::new(LeaseRegistry::new(clock.clone())),
      clock.clone(),
    );
    Self {
      engine,
      clock,
      effects,
      binding: RunBinding {
        daemon_session_id: Uuid::new_v4().to_string(),
        run_id: Uuid::new_v4().to_string(),
        owner_epoch: 1,
      },
    }
  }
  pub fn call(&self, operation: Operation) -> RuntimeResult<Reply> {
    self.as_binding(self.binding.clone(), operation)
  }
  pub fn as_binding(&self, binding: RunBinding, operation: Operation) -> RuntimeResult<Reply> {
    self
      .engine
      .handle(Command { binding, operation }, CancellationToken::new())
  }
  pub fn lease(&self) -> LeaseToken {
    match self
      .call(Operation::AcquireLease { ttl_ms: 300_000 })
      .expect("acquire")
    {
      Reply::Lease(lease) => lease,
      _ => panic!("lease reply"),
    }
  }
  pub fn snapshot(&self, lease: &LeaseToken) -> DesktopSnapshot {
    match self
      .call(Operation::Observe {
        lease: lease.clone(),
        limits: ObservationLimits {
          max_pixels: 4,
          max_bytes: 16,
          ..ObservationLimits::default()
        },
        timeout_ms: 1000,
      })
      .expect("observe")
    {
      Reply::Snapshot(snapshot) => snapshot,
      _ => panic!("snapshot reply"),
    }
  }
  pub fn request(&self, lease: &LeaseToken, snapshot: &DesktopSnapshot) -> DesktopActionRequest {
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
  pub fn prepare(&self, lease: &LeaseToken) -> Preparation {
    let request = self.request(lease, &self.snapshot(lease));
    match self.call(Operation::Prepare { request }).expect("prepare") {
      Reply::Prepared(preparation) => preparation,
      _ => panic!("preparation reply"),
    }
  }
  pub fn input(&self) -> usize {
    self.effects.lock().expect("effects").input
  }
}
pub fn execution(preparation: &Preparation) -> Operation {
  Operation::Execute {
    preparation_id: preparation.preparation_id,
    action_id: preparation.request.action_id,
    content_digest: preparation.content_digest,
    approval_id: Uuid::new_v4(),
    timeout_ms: 1000,
  }
}
