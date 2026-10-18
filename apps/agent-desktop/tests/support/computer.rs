use agent_computer::*;
use agent_computer_runtime::{
  ComputerHostFactory, ComputerRuntime, RuntimeAttachment, attach_host,
};
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct Factory {
  pub attached: AtomicUsize,
  pub dropped: Arc<AtomicUsize>,
}

pub fn target() -> DesktopTarget {
  DesktopTarget {
    scope: TargetScope::Window(7),
    label: "Private fixture window".into(),
    process_id: Some(42),
    bounds: Bounds {
      x: 0,
      y: 0,
      width: 2,
      height: 2,
    },
  }
}

#[async_trait::async_trait]
impl ComputerHostFactory for Factory {
  fn available(&self) -> bool {
    true
  }
  async fn targets(&self) -> anyhow::Result<Vec<DesktopTarget>> {
    Ok(vec![target()])
  }
  async fn attach(
    &self,
    client: agent_client::AgentClient,
    session: &str,
    target: DesktopTarget,
  ) -> anyhow::Result<RuntimeAttachment> {
    let node = client.node().await?;
    let desktop = DesktopSession {
      id: uuid::Uuid::now_v7(),
      identity: DesktopIdentity {
        node_id: node.id,
        user_session: "synthetic-desktop".into(),
      },
      scope: target.scope,
    };
    let runtime = Arc::new(ComputerRuntime::new(
      desktop.clone(),
      Box::new(Backend {
        identity: desktop.identity,
        dropped: self.dropped.clone(),
      }),
      Arc::new(LeaseRegistry::default()),
    ));
    let host = attach_host(client, session, runtime).await?;
    self.attached.fetch_add(1, Ordering::SeqCst);
    Ok(host)
  }
}

struct Backend {
  identity: DesktopIdentity,
  dropped: Arc<AtomicUsize>,
}
impl Drop for Backend {
  fn drop(&mut self) {
    self.dropped.fetch_add(1, Ordering::SeqCst);
  }
}
impl DesktopBackend for Backend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    Ok(DesktopCapabilities {
      identity: self.identity.clone(),
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
    Err(DesktopError::Unsupported("fixture never captures".into()))
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    Err(DesktopError::StaleSnapshot)
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    _: &ExecutionGuard,
  ) -> ActionOutcome {
    ActionOutcome::Rejected(DesktopError::Unsupported("fixture never inputs".into()))
  }
}
