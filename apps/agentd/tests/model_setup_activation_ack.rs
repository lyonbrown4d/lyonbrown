mod model_setup_support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{ModelSetupAction, ModelSetupPhase, RuntimeCommandError, spawn_runtime};
use agent_kernel::Kernel;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use agentd::epoch::{ActivationAckGate, CutoverOutcome, DescriptorPublisher, EpochSupervisor};
use agentd::model_setup::PreparedRuntime;
use anyhow::{Result, anyhow};
use tokio::time::timeout;

use model_setup_support::{Harness, InstallMode, preset_id};

#[derive(Default)]
struct Publisher {
  current: Mutex<Option<DiscoveryDescriptor>>,
  reject_next: Mutex<bool>,
}

impl Publisher {
  fn reject_next(&self) {
    *self.reject_next.lock().unwrap() = true;
  }
}

impl DescriptorPublisher for Publisher {
  fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<()> {
    if std::mem::take(&mut *self.reject_next.lock().unwrap()) {
      return Err(anyhow!("injected publication failure"));
    }
    *self.current.lock().unwrap() = Some(descriptor.clone());
    Ok(())
  }

  fn read(&self) -> Result<DiscoveryDescriptor> {
    self
      .current
      .lock()
      .unwrap()
      .clone()
      .ok_or_else(|| anyhow!("not published"))
  }
}

async fn supervisor(harness: &Harness, publisher: Arc<Publisher>) -> EpochSupervisor {
  let mut supervisor = EpochSupervisor::new(
    publisher,
    harness.coordinator.clone(),
    ScopeFingerprint::parse(&"34".repeat(32)).unwrap(),
    Duration::from_millis(250),
  );
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  supervisor
    .start_initial(PreparedRuntime::new(controller, runtime))
    .await
    .unwrap();
  supervisor
}

async fn replacement(harness: &mut Harness) -> PreparedRuntime {
  timeout(Duration::from_secs(2), harness.replacements.recv())
    .await
    .unwrap()
    .unwrap()
}

async fn stop(prepared: PreparedRuntime) {
  prepared.runtime.shutdown_handle().request_shutdown();
  drop(prepared.controller);
  prepared.runtime.join().await.unwrap();
}

#[tokio::test]
async fn matching_success_ack_publishes_ready_and_clears_the_operation() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let prepared = replacement(&mut harness).await;
  let operation_id = prepared.activation_operation_id().unwrap().to_owned();

  assert!(
    harness
      .coordinator
      .activation_succeeded(&operation_id)
      .await
  );
  let ready = harness.coordinator.snapshot().unwrap();
  assert_eq!(ready.phase, ModelSetupPhase::Ready);
  assert_eq!(ready.operation_id, None);
  assert!(ready.allowed_actions.is_empty());
  assert_eq!(
    harness.coordinator.start(preset_id().into()).await,
    Err(RuntimeCommandError::ModelDownloadUnavailable)
  );
  assert!(
    !harness
      .coordinator
      .activation_succeeded(&operation_id)
      .await
  );
  stop(prepared).await;
}

#[tokio::test]
async fn failure_ack_is_retryable_and_stale_ack_cannot_overwrite_a_retry() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  assert!(
    !harness
      .coordinator
      .activation_failed("no-active", "activation_unavailable")
      .await
  );
  assert_eq!(
    harness.coordinator.snapshot().unwrap().phase,
    ModelSetupPhase::Required
  );

  harness.coordinator.start(preset_id().into()).await.unwrap();
  let first = replacement(&mut harness).await;
  let first_id = first.activation_operation_id().unwrap().to_owned();
  assert!(
    harness
      .coordinator
      .activation_failed(&first_id, "activation_unavailable")
      .await
  );
  let failed = harness.coordinator.snapshot().unwrap();
  assert_eq!(failed.phase, ModelSetupPhase::Failed);
  assert_eq!(failed.allowed_actions, vec![ModelSetupAction::Retry]);
  assert_eq!(failed.failure.unwrap().code, "activation_unavailable");

  harness.coordinator.start(preset_id().into()).await.unwrap();
  let second = replacement(&mut harness).await;
  let second_id = second.activation_operation_id().unwrap().to_owned();
  assert_ne!(first_id, second_id);
  assert!(!harness.coordinator.activation_succeeded(&first_id).await);
  let activating = harness.coordinator.snapshot().unwrap();
  assert_eq!(activating.phase, ModelSetupPhase::Activating);
  assert_eq!(activating.operation_id.as_deref(), Some(second_id.as_str()));

  assert!(
    harness
      .coordinator
      .activation_failed(&second_id, "epoch_publication_failed")
      .await
  );
  assert_eq!(
    harness
      .coordinator
      .snapshot()
      .unwrap()
      .failure
      .unwrap()
      .code,
    "epoch_publication_failed"
  );
  stop(first).await;
  stop(second).await;
}

#[tokio::test]
async fn supervisor_acknowledges_success_only_after_epoch_publication() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  let publisher = Arc::new(Publisher::default());
  let mut supervisor = supervisor(&harness, publisher).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let prepared = replacement(&mut harness).await;

  assert_eq!(
    supervisor.cutover(prepared).await.unwrap(),
    CutoverOutcome::Switched
  );
  assert_eq!(
    harness.coordinator.snapshot().unwrap().phase,
    ModelSetupPhase::Ready
  );
  supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn supervisor_acknowledges_retryable_failure_when_publication_is_rejected() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  let publisher = Arc::new(Publisher::default());
  let mut supervisor = supervisor(&harness, publisher.clone()).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let prepared = replacement(&mut harness).await;
  publisher.reject_next();

  assert_eq!(
    supervisor.cutover(prepared).await.unwrap(),
    CutoverOutcome::KeptCurrent
  );
  let failed = harness.coordinator.snapshot().unwrap();
  assert_eq!(failed.phase, ModelSetupPhase::Failed);
  assert_eq!(failed.failure.unwrap().code, "epoch_publication_failed");
  supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_wins_over_a_ready_cutover_and_disposes_the_unpublished_runtime() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  let publisher = Arc::new(Publisher::default());
  let mut supervisor = supervisor(&harness, publisher.clone()).await;
  let initial = publisher.read().unwrap();
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let prepared = replacement(&mut harness).await;
  let controller = prepared.controller.clone();
  let (sender, mut replacements) = tokio::sync::mpsc::unbounded_channel();
  sender.send(prepared).unwrap();

  supervisor
    .supervise(&mut replacements, async {})
    .await
    .unwrap();

  assert_eq!(publisher.read().unwrap(), initial);
  assert_eq!(
    controller
      .execute(agent_app::AgentCommand::StartTurn { text: "x".into() })
      .await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}

#[tokio::test]
async fn stale_prepared_runtime_is_rolled_back_without_stopping_the_current_epoch() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  let publisher = Arc::new(Publisher::default());
  let mut supervisor = supervisor(&harness, publisher.clone()).await;
  let initial = publisher.read().unwrap();
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let stale = replacement(&mut harness).await;
  let stale_id = stale.activation_operation_id().unwrap().to_owned();
  assert!(
    harness
      .coordinator
      .activation_failed(&stale_id, "activation_unavailable")
      .await
  );
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let current = replacement(&mut harness).await;
  let current_id = current.activation_operation_id().unwrap().to_owned();

  assert_eq!(
    supervisor.cutover(stale).await.unwrap(),
    CutoverOutcome::KeptCurrent
  );
  assert_eq!(publisher.read().unwrap(), initial);
  assert_eq!(
    harness
      .coordinator
      .snapshot()
      .unwrap()
      .operation_id
      .as_deref(),
    Some(current_id.as_str())
  );
  supervisor.shutdown().await.unwrap();
  stop(current).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_exit_after_success_ack_is_compensated_and_rolled_back() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  let publisher = Arc::new(Publisher::default());
  let gate = Arc::new(ActivationAckGate::default());
  let supervisor = supervisor(&harness, publisher.clone())
    .await
    .with_activation_ack_gate(gate.clone());
  let initial = publisher.read().unwrap();
  harness.coordinator.start(preset_id().into()).await.unwrap();
  let prepared = replacement(&mut harness).await;
  let runtime = prepared.runtime.shutdown_handle();
  let cutover = tokio::spawn(async move {
    let mut supervisor = supervisor;
    let outcome = supervisor.cutover(prepared).await;
    (supervisor, outcome)
  });
  timeout(Duration::from_secs(2), gate.wait_until_entered())
    .await
    .expect("cutover must pause after its success ACK");
  runtime.abort();
  gate.release();
  let (mut supervisor, outcome) = cutover.await.unwrap();

  assert_eq!(outcome.unwrap(), CutoverOutcome::KeptCurrent);
  assert_eq!(publisher.read().unwrap(), initial);
  let failed = harness.coordinator.snapshot().unwrap();
  assert_eq!(failed.phase, ModelSetupPhase::Failed);
  assert_eq!(failed.failure.unwrap().code, "epoch_replacement_exited");
  supervisor.shutdown().await.unwrap();
}
