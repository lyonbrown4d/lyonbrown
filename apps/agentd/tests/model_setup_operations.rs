mod model_setup_support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use agent_app::{AgentCommand, ModelSetupAction, ModelSetupPhase, RuntimeCommandError};
use tokio::time::timeout;

use model_setup_support::{Harness, InstallMode, preset_id};

async fn wait_for_phase(harness: &Harness, expected: ModelSetupPhase) {
  let mut snapshots = harness.coordinator.subscribe();
  timeout(Duration::from_secs(2), async {
    loop {
      if snapshots
        .borrow()
        .as_ref()
        .is_some_and(|view| view.phase == expected)
      {
        break;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn start_returns_before_the_background_install_finishes_and_deduplicates() {
  let harness = Harness::managed(InstallMode::WaitForCancel, false).await;

  timeout(
    Duration::from_millis(100),
    harness.coordinator.start(preset_id().into()),
  )
  .await
  .unwrap()
  .unwrap();
  harness.installer.wait_started().await;
  harness.coordinator.start(preset_id().into()).await.unwrap();

  assert_eq!(harness.installer.calls.load(Ordering::SeqCst), 1);
  let view = harness.coordinator.snapshot().unwrap();
  assert_eq!(view.phase, ModelSetupPhase::WaitingForLock);
  assert_eq!(view.allowed_actions, vec![ModelSetupAction::Cancel]);
}

#[tokio::test]
async fn cancel_requires_the_current_operation_and_publishes_cancelled() {
  let harness = Harness::managed(InstallMode::WaitForCancel, false).await;
  assert_eq!(
    harness.coordinator.cancel("none".into()).await,
    Err(RuntimeCommandError::NoActiveModelDownload)
  );
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();

  assert_eq!(
    harness.coordinator.cancel("stale-operation".into()).await,
    Err(RuntimeCommandError::ModelDownloadOperationMismatch {
      expected: operation_id.clone(),
      received: "stale-operation".into(),
    })
  );
  harness.coordinator.cancel(operation_id).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;

  let view = harness.coordinator.snapshot().unwrap();
  assert_eq!(view.allowed_actions, vec![ModelSetupAction::Retry]);
  assert_eq!(harness.installer.cancellations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_is_rejected_after_installing_begins() {
  let harness = Harness::managed(InstallMode::Installing, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  wait_for_phase(&harness, ModelSetupPhase::Installing).await;
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();

  assert_eq!(
    harness.coordinator.cancel(operation_id).await,
    Err(RuntimeCommandError::ModelDownloadCancellationTooLate)
  );
  assert!(
    harness
      .coordinator
      .snapshot()
      .unwrap()
      .allowed_actions
      .is_empty()
  );
}

#[tokio::test]
async fn cancel_accepted_before_install_gate_prevents_entering_installing() {
  let harness = Harness::managed(InstallMode::InstallingAfterRelease, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();

  harness.coordinator.cancel(operation_id).await.unwrap();
  harness.installer.release_one();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;

  assert_ne!(
    harness.coordinator.snapshot().unwrap().phase,
    ModelSetupPhase::Installing
  );
  assert_eq!(harness.activator.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn closed_progress_channel_does_not_starve_the_installer_future() {
  let harness = Harness::managed(InstallMode::CloseProgressWaitForCancel, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();

  timeout(
    Duration::from_millis(200),
    harness.coordinator.cancel(operation_id),
  )
  .await
  .unwrap()
  .unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;
  assert_eq!(harness.installer.cancellations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn accepted_cancel_wins_when_installer_returns_success_after_cancellation() {
  let mut harness = Harness::managed(InstallMode::SuccessAfterCancel, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();

  harness.coordinator.cancel(operation_id).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;

  assert_eq!(harness.activator.calls.load(Ordering::SeqCst), 0);
  assert!(harness.replacements.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_publication_cannot_overwrite_a_concurrent_retry() {
  let harness = Harness::managed(InstallMode::FailureAfterReleaseThenWait, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  harness.installer.release_one();

  timeout(Duration::from_secs(2), async {
    loop {
      harness.coordinator.start(preset_id().into()).await.unwrap();
      if harness.installer.calls.load(Ordering::SeqCst) == 2 {
        break;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  harness.installer.wait_started().await;

  let view = harness.coordinator.snapshot().unwrap();
  assert_eq!(view.phase, ModelSetupPhase::WaitingForLock);
  let operation_id = view.operation_id.unwrap();
  harness.coordinator.cancel(operation_id).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;
}

#[tokio::test]
async fn dropping_a_snapshot_subscriber_does_not_cancel_background_work() {
  let harness = Harness::managed(InstallMode::WaitForCancel, false).await;
  drop(harness.coordinator.subscribe());
  harness.coordinator.start(preset_id().into()).await.unwrap();
  harness.installer.wait_started().await;
  tokio::task::yield_now().await;

  assert_eq!(harness.installer.cancellations.load(Ordering::SeqCst), 0);
  let operation_id = harness
    .coordinator
    .snapshot()
    .unwrap()
    .operation_id
    .unwrap();
  harness.coordinator.cancel(operation_id).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Cancelled).await;
}

#[tokio::test]
async fn successful_install_activates_and_emits_a_prepared_replacement() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();

  let prepared = timeout(Duration::from_secs(2), harness.replacements.recv())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    harness.coordinator.snapshot().unwrap().phase,
    ModelSetupPhase::Activating
  );
  assert_eq!(harness.activator.calls.load(Ordering::SeqCst), 1);
  prepared.runtime.shutdown_handle().request_shutdown();
  drop(prepared.controller);
  prepared.runtime.join().await.unwrap();
}

#[tokio::test]
async fn rejected_replacement_is_shutdown_and_joined_before_failure_is_published() {
  let mut harness = Harness::managed(InstallMode::Success, false).await;
  harness.replacements.close();
  harness.coordinator.start(preset_id().into()).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Failed).await;

  let controller = harness.activator.last_controller().unwrap();
  let result = timeout(
    Duration::from_millis(200),
    controller.execute(AgentCommand::StartTurn {
      text: "must not reach a detached runtime".into(),
    }),
  )
  .await
  .unwrap();
  assert_eq!(result, Err(RuntimeCommandError::RuntimeStopped));
  assert_eq!(
    harness
      .coordinator
      .snapshot()
      .unwrap()
      .failure
      .unwrap()
      .code,
    "activation_unavailable"
  );
}

#[tokio::test]
async fn activation_failure_is_retryable_and_does_not_expose_internal_errors() {
  let harness = Harness::managed(InstallMode::Success, true).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Failed).await;

  let view = harness.coordinator.snapshot().unwrap();
  assert_eq!(view.allowed_actions, vec![ModelSetupAction::Retry]);
  let failure = view.failure.unwrap();
  assert!(failure.retryable);
  assert_eq!(failure.code, "activation_failed");
  assert!(!failure.message.contains("secret-token"));
  assert!(!failure.message.contains("C:/private"));
  assert_eq!(
    harness.coordinator.cancel("old".into()).await,
    Err(RuntimeCommandError::NoActiveModelDownload)
  );
}

#[tokio::test]
async fn installer_failure_is_sanitized_and_retryable() {
  let harness = Harness::managed(InstallMode::Failure, false).await;
  harness.coordinator.start(preset_id().into()).await.unwrap();
  wait_for_phase(&harness, ModelSetupPhase::Failed).await;

  let failure = harness.coordinator.snapshot().unwrap().failure.unwrap();
  assert_eq!(failure.code, "download_failed");
  assert!(failure.retryable);
  assert!(!failure.message.contains("secret-token"));
  assert!(!failure.message.contains("C:/private"));
}
