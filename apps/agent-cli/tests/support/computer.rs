use std::sync::{
  Arc, Mutex,
  atomic::{AtomicU32, AtomicUsize, Ordering},
};

use agent_client::AgentClient;
use agent_computer::*;
use agent_computer_runtime::{
  ComputerHostFactory, ComputerRuntime, DesktopTarget, RuntimeAttachment,
};

#[derive(Default)]
pub struct Factory {
  pub attachments: Mutex<Vec<(String, TargetScope)>>,
  pub selected: Mutex<Vec<DesktopTarget>>,
  pub generation: AtomicU32,
  pub effects: Arc<Effects>,
}
#[derive(Default)]
pub struct Effects {
  pub captures: AtomicUsize,
  pub inputs: AtomicUsize,
  pub drops: AtomicUsize,
}
struct Backend {
  identity: DesktopIdentity,
  effects: Arc<Effects>,
}
impl Drop for Backend {
  fn drop(&mut self) {
    self.effects.drops.fetch_add(1, Ordering::SeqCst);
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
    self.effects.captures.fetch_add(1, Ordering::SeqCst);
    Err(DesktopError::BackendUnavailable(
      "unexpected fixture capture".into(),
    ))
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    Ok(())
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    _: &ExecutionGuard,
  ) -> ActionOutcome {
    self.effects.inputs.fetch_add(1, Ordering::SeqCst);
    ActionOutcome::Rejected(DesktopError::BackendUnavailable(
      "unexpected fixture input".into(),
    ))
  }
}
fn target(scope: TargetScope, generation: u32) -> DesktopTarget {
  DesktopTarget {
    scope,
    label: "Synthetic local target".into(),
    process_id: Some(19 + generation),
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
    Ok(vec![
      target(
        TargetScope::Window(7),
        self.generation.load(Ordering::SeqCst),
      ),
      target(
        TargetScope::Display(2),
        self.generation.load(Ordering::SeqCst),
      ),
    ])
  }
  async fn attach(
    &self,
    client: AgentClient,
    session: &str,
    target: DesktopTarget,
  ) -> anyhow::Result<RuntimeAttachment> {
    self.selected.lock().unwrap().push(target.clone());
    if target.process_id != Some(19 + self.generation.load(Ordering::SeqCst)) {
      return Err(DesktopError::StaleSnapshot.into());
    }
    let identity = DesktopIdentity {
      node_id: client.node().await?.id,
      user_session: "synthetic-local-user".into(),
    };
    let runtime = Arc::new(ComputerRuntime::new(
      DesktopSession {
        id: uuid::Uuid::now_v7(),
        identity: identity.clone(),
        scope: target.scope.clone(),
      },
      Box::new(Backend {
        identity,
        effects: self.effects.clone(),
      }),
      Arc::new(LeaseRegistry::default()),
    ));
    let attachment = agent_computer_runtime::attach_host(client, session, runtime).await?;
    self
      .attachments
      .lock()
      .unwrap()
      .push((session.into(), target.scope));
    Ok(attachment)
  }
}
