mod model_setup_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agent_app::{
  AgentCommand, ModelSetupPhase, RuntimeCommandError, RuntimePhase, RuntimeSnapshot,
};
use agentd::{
  CAPABILITY_MODEL_SETUP_DOWNLOAD_V1, ManagedRuntimeHost, RuntimeHost, VersionedSnapshot,
};
use async_trait::async_trait;
use tokio::sync::watch;
use tokio::time::timeout;

use model_setup_support::{Harness, InstallMode, preset_id, waiting_snapshot};

struct FakeHost {
  snapshots: watch::Sender<VersionedSnapshot>,
  forwarded: AtomicUsize,
}

impl FakeHost {
  fn new() -> Arc<Self> {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(7, waiting_snapshot()));
    Arc::new(Self {
      snapshots,
      forwarded: AtomicUsize::new(0),
    })
  }

  fn publish(&self, revision: u64, phase: RuntimePhase) {
    self.snapshots.send_replace(VersionedSnapshot::new(
      revision,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    ));
  }
}

#[async_trait]
impl RuntimeHost for FakeHost {
  async fn execute(&self, _command: AgentCommand) -> Result<(), RuntimeCommandError> {
    self.forwarded.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

#[tokio::test]
async fn managed_host_merges_both_sources_into_one_monotonic_stream() {
  let harness = Harness::managed(InstallMode::Success, true).await;
  let inner = FakeHost::new();
  let host = ManagedRuntimeHost::new(inner.clone(), harness.coordinator.clone());
  let mut snapshots = host.subscribe();
  let initial = snapshots.borrow().clone();

  assert_eq!(initial.revision, 7);
  assert_eq!(
    initial.snapshot.model_setup.unwrap().phase,
    ModelSetupPhase::Required
  );
  assert_eq!(host.capabilities(), &[CAPABILITY_MODEL_SETUP_DOWNLOAD_V1]);

  inner.publish(8, RuntimePhase::Ready);
  snapshots.changed().await.unwrap();
  let after_runtime = snapshots.borrow_and_update().clone();
  assert!(after_runtime.revision > initial.revision);
  assert_eq!(after_runtime.snapshot.phase, RuntimePhase::Ready);

  host
    .execute(AgentCommand::StartModelDownload {
      model_id: preset_id().into(),
    })
    .await
    .unwrap();
  timeout(Duration::from_secs(2), async {
    loop {
      snapshots.changed().await.unwrap();
      if snapshots
        .borrow()
        .snapshot
        .model_setup
        .as_ref()
        .is_some_and(|setup| setup.phase == ModelSetupPhase::Failed)
      {
        break;
      }
    }
  })
  .await
  .unwrap();
  let after_setup = snapshots.borrow().clone();
  assert!(after_setup.revision > after_runtime.revision);

  host
    .execute(AgentCommand::StartTurn {
      text: "control plane remains usable".into(),
    })
    .await
    .unwrap();
  assert_eq!(inner.forwarded.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_late_subscriber_waits_for_a_strictly_newer_revision() {
  let harness = Harness::managed(InstallMode::WaitForCancel, false).await;
  let inner = FakeHost::new();
  let host = ManagedRuntimeHost::new(inner.clone(), harness.coordinator.clone());
  let initial_revision = host.snapshot().revision;
  inner.publish(8, RuntimePhase::Ready);
  timeout(Duration::from_secs(2), async {
    while host.snapshot().revision == initial_revision {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  let current_revision = host.snapshot().revision;
  let mut late = host.subscribe();

  assert!(
    timeout(Duration::from_millis(20), late.changed())
      .await
      .is_err()
  );
  inner.publish(9, RuntimePhase::Preparing);
  timeout(Duration::from_secs(2), late.changed())
    .await
    .unwrap()
    .unwrap();
  assert!(late.borrow().revision > current_revision);
}
