use agent_computer::*;
use agent_computer_runtime::*;
use std::sync::{
  Arc, Condvar, Mutex,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
pub struct Gate {
  released: Mutex<bool>,
  wake: Condvar,
  pub entered: Notify,
  pub finished: Notify,
  pub inputs: AtomicUsize,
  pub cancelled: AtomicBool,
}
impl Gate {
  pub fn release(&self) {
    *self.released.lock().expect("gate") = true;
    self.wake.notify_all();
  }
  pub fn input_count(&self) -> usize {
    self.inputs.load(Ordering::SeqCst)
  }
}
struct Backend {
  session: DesktopSession,
  gate: Arc<Gate>,
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
    _: ObservationLimits,
    _: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
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
        rgba: vec![0; 16],
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
    Ok(())
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    self.gate.entered.notify_one();
    let released = self.gate.released.lock().expect("gate");
    let _released = self
      .gate
      .wake
      .wait_timeout_while(released, std::time::Duration::from_secs(3), |released| {
        !*released
      })
      .expect("gate wait");
    let outcome = match guard.checkpoint() {
      Ok(()) => {
        self.gate.inputs.fetch_add(1, Ordering::SeqCst);
        ActionOutcome::Completed
      }
      Err(error) => {
        self.gate.cancelled.store(true, Ordering::SeqCst);
        ActionOutcome::Rejected(error)
      }
    };
    self.gate.finished.notify_one();
    outcome
  }
}
pub struct Host {
  pub runtime: Arc<ComputerRuntime>,
  pub gate: Arc<Gate>,
  pub binding: RunBinding,
  preparation: Preparation,
}
impl Host {
  pub fn new(node: &str, session: &str) -> Self {
    let desktop = DesktopSession {
      id: Uuid::new_v4(),
      identity: DesktopIdentity {
        node_id: node.into(),
        user_session: "synthetic-native-session".into(),
      },
      scope: TargetScope::Window(7),
    };
    let gate = Arc::new(Gate::default());
    let runtime = Arc::new(ComputerRuntime::new(
      desktop.clone(),
      Box::new(Backend {
        session: desktop,
        gate: gate.clone(),
      }),
      Arc::new(LeaseRegistry::default()),
    ));
    let binding = RunBinding {
      daemon_session_id: session.into(),
      run_id: Uuid::new_v4().to_string(),
      owner_epoch: 1,
    };
    let call = |operation| {
      runtime
        .handle(
          Command {
            binding: binding.clone(),
            operation,
          },
          CancellationToken::new(),
        )
        .expect("fixture preparation")
    };
    let Reply::Lease(lease) = call(Operation::AcquireLease { ttl_ms: 300_000 }) else {
      panic!("lease reply")
    };
    let Reply::Snapshot(snapshot) = call(Operation::Observe {
      lease: lease.clone(),
      limits: ObservationLimits {
        max_pixels: 4,
        max_bytes: 16,
        ..Default::default()
      },
      timeout_ms: 1000,
    }) else {
      panic!("snapshot reply")
    };
    let Reply::Prepared(preparation) = call(Operation::Prepare {
      request: DesktopActionRequest {
        action_id: Uuid::new_v4(),
        lease,
        snapshot_id: snapshot.id,
        snapshot_revision: snapshot.revision,
        action: DesktopAction::Click {
          x: 1,
          y: 1,
          button: MouseButton::Left,
        },
      },
    }) else {
      panic!("preparation reply")
    };
    Self {
      runtime,
      gate,
      binding,
      preparation,
    }
  }
  pub fn execute(&self) -> Operation {
    Operation::Execute {
      preparation_id: self.preparation.preparation_id,
      action_id: self.preparation.request.action_id,
      content_digest: self.preparation.content_digest,
      approval_id: Uuid::new_v4(),
      timeout_ms: 2000,
    }
  }
  pub fn request(
    &self,
    id: Uuid,
    operation: Operation,
  ) -> agent_protocol::v1::ComputerRuntimeRequest {
    agent_protocol::v1::ComputerRuntimeRequest {
      request_id: id.to_string(),
      payload: wire::encode(&Command {
        binding: self.binding.clone(),
        operation,
      })
      .expect("fixture packet"),
    }
  }
}
