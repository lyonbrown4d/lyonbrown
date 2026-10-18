mod support;

use std::{
  sync::{Arc, Barrier, mpsc},
  thread,
  time::Duration,
};

use agent_computer::*;
use support::*;
use uuid::Uuid;

#[test]
fn expired_controller_cannot_revoke_replacement_lease() {
  let clock = Arc::new(Clock::new());
  let registry = LeaseRegistry::new(clock.clone());
  let session = session();
  let old = registry
    .acquire(&session, Duration::from_secs(1))
    .expect("old lease");
  clock.advance(1000);
  let next = registry
    .acquire(&session, Duration::from_secs(10))
    .expect("replacement");
  assert_eq!(registry.revoke_token(&old), Err(DesktopError::LeaseInvalid));
  assert_eq!(registry.validate(&next), Ok(()));
  registry.revoke_token(&next).expect("revoke current");
  assert_eq!(registry.validate(&next), Err(DesktopError::LeaseInvalid));
}

#[test]
fn two_windows_in_one_graphical_session_have_one_effective_writer() {
  let registry = Arc::new(LeaseRegistry::default());
  let first = session();
  let second = DesktopSession {
    id: Uuid::new_v4(),
    scope: TargetScope::Window(99),
    ..first.clone()
  };
  let barrier = Arc::new(Barrier::new(3));
  let workers: Vec<_> = [first, second]
    .into_iter()
    .map(|session| {
      let registry = registry.clone();
      let barrier = barrier.clone();
      thread::spawn(move || {
        barrier.wait();
        registry.acquire(&session, Duration::from_secs(5))
      })
    })
    .collect();
  barrier.wait();
  let results: Vec<_> = workers
    .into_iter()
    .map(|worker| worker.join().expect("worker"))
    .collect();
  assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
  assert_eq!(
    results
      .iter()
      .filter(|r| **r == Err(DesktopError::LeaseBusy))
      .count(),
    1
  );
}

struct BlockingBackend {
  inner: Backend,
  entered: mpsc::SyncSender<()>,
  resume: mpsc::Receiver<()>,
}

impl DesktopBackend for BlockingBackend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    self.inner.capabilities()
  }
  fn observe(
    &mut self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    self.inner.observe(session, limits, control)
  }
  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.inner.validate_snapshot(snapshot)
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    self.inner.effects.lock().expect("effects").calls += 1;
    self.entered.send(()).expect("entered");
    self.resume.recv().expect("resume");
    match guard.checkpoint() {
      Ok(()) => ActionOutcome::Completed,
      Err(error) => ActionOutcome::Uncertain(error),
    }
  }
}

#[test]
fn human_revoke_is_immediate_and_takeover_waits_for_dispatched_os_step() {
  let f = Fixture::new();
  let (entered_tx, entered_rx) = mpsc::sync_channel(1);
  let (resume_tx, resume_rx) = mpsc::sync_channel(1);
  let backend = BlockingBackend {
    inner: Backend {
      session: f.session.clone(),
      effects: f.effects.clone(),
    },
    entered: entered_tx,
    resume: resume_rx,
  };
  let runtime = Arc::new(DesktopRuntime::new(
    f.session.clone(),
    f.leases.clone(),
    Box::new(backend),
  ));
  let lease = runtime
    .acquire_lease(Duration::from_secs(30))
    .expect("lease");
  let snapshot = runtime
    .observe(&lease, ObservationLimits::default(), f.control())
    .expect("snapshot");
  let request = request(&lease, &snapshot);
  let action_id = request.action_id;
  let worker = {
    let runtime = runtime.clone();
    let control = f.control();
    thread::spawn(move || runtime.execute(approved(request), control))
  };
  entered_rx
    .recv_timeout(Duration::from_secs(5))
    .expect("OS step entered");
  // revoke does not acquire the backend mutex and must return before resume.
  runtime.revoke().expect("human revoke");
  assert_eq!(f.leases.validate(&lease), Err(DesktopError::LeaseInvalid));
  assert_eq!(
    f.leases.acquire(&f.session, Duration::from_secs(1)),
    Err(DesktopError::LeaseBusy)
  );
  resume_tx.send(()).expect("resume");
  let receipt = worker.join().expect("worker").expect("receipt");
  assert_eq!(
    receipt.outcome,
    ActionOutcome::Uncertain(DesktopError::LeaseInvalid)
  );
  assert_eq!(runtime.receipt(action_id).expect("receipt"), Some(receipt));
  let next = runtime
    .acquire_lease(Duration::from_secs(1))
    .expect("takeover after drain");
  assert_ne!(next.id, lease.id);
  assert_eq!(f.effects.lock().expect("effects").calls, 1);
}
