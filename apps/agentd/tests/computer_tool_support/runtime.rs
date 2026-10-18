use crate::support::TestNode;
use agent_computer::*;
use agent_computer_runtime::{ComputerRuntime, RuntimeAttachment, attach_host};
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Default)]
pub struct Calls {
  pub input_denied: AtomicBool,
  pub content_changed: AtomicBool,
  pub captures: AtomicUsize,
  pub effects: Mutex<Vec<DesktopAction>>,
  pub executed: Notify,
}
pub struct Runtime {
  pub engine: Arc<ComputerRuntime>,
  pub calls: Arc<Calls>,
}
impl Runtime {
  pub fn new(node: &TestNode) -> Self {
    let calls = Arc::new(Calls::default());
    let session = DesktopSession {
      id: Uuid::new_v4(),
      identity: DesktopIdentity {
        node_id: node.daemon.node_id.clone(),
        user_session: "synthetic-model-computer-user".into(),
      },
      scope: TargetScope::Window(7),
    };
    let backend = Backend {
      identity: session.identity.clone(),
      calls: calls.clone(),
    };
    Self {
      engine: Arc::new(ComputerRuntime::new(
        session,
        Box::new(backend),
        Arc::new(LeaseRegistry::default()),
      )),
      calls,
    }
  }
  pub async fn attach(&self, node: &TestNode) -> RuntimeAttachment {
    attach_host(node.client.clone(), &node.session_id, self.engine.clone())
      .await
      .unwrap()
  }
  pub fn effects(&self) -> usize {
    self.calls.effects.lock().unwrap().len()
  }
  pub async fn wait_effect(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      loop {
        let changed = self.calls.executed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.effects() > 0 {
          return;
        }
        changed.await;
      }
    })
    .await
    .unwrap();
  }
}
struct Backend {
  identity: DesktopIdentity,
  calls: Arc<Calls>,
}
impl DesktopBackend for Backend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    Ok(DesktopCapabilities {
      identity: self.identity.clone(),
      interactive: true,
      capture: PermissionState::Granted,
      input: if self.calls.input_denied.load(Ordering::SeqCst) {
        PermissionState::Denied
      } else {
        PermissionState::Granted
      },
      accessibility: PermissionState::Granted,
    })
  }
  fn observe(
    &mut self,
    _: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    control.checkpoint(std::time::Instant::now())?;
    limits.check_image(80, 80)?;
    self.calls.captures.fetch_add(1, Ordering::SeqCst);
    let mut observed = observation();
    if self.calls.content_changed.load(Ordering::SeqCst) {
      observed.screenshot.rgba[0] = 128;
    }
    Ok(observed)
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    Ok(())
  }
  fn execute(
    &mut self,
    action: &DesktopAction,
    _: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    if let Err(error) = guard.checkpoint() {
      return ActionOutcome::Rejected(error);
    }
    self.calls.effects.lock().unwrap().push(action.clone());
    self.calls.executed.notify_waiters();
    ActionOutcome::Completed
  }
}
fn observation() -> DesktopObservation {
  let bounds = Bounds {
    x: 0,
    y: 0,
    width: 80,
    height: 80,
  };
  DesktopObservation {
    bounds,
    screenshot: Screenshot {
      width: 80,
      height: 80,
      rgba: vec![127; 80 * 80 * 4],
    },
    windows: vec![WindowObservation {
      id: 7,
      process_id: 42,
      title: "Untrusted fixture window".into(),
      bounds,
      focused: true,
    }],
    windows_truncated: false,
    accessibility: AccessibilityObservation {
      state: PermissionState::Granted,
      nodes: vec![AccessibilityNode {
        index: 0,
        parent: None,
        role: "button".into(),
        label: "Synthetic submit".into(),
      }],
      truncated: false,
    },
  }
}
