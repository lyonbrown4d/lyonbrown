use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentCommand, RuntimeCommandError, RuntimePhase, spawn_runtime};
use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use provider_mock::{MockProvider, ScriptedStep};
use tokio::sync::watch;

fn pending_kernel() -> Kernel {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::pending(),
  ])));
  Kernel::builder().providers(providers).build()
}

async fn wait_for_phase(
  snapshots: &mut watch::Receiver<agent_app::RuntimeSnapshot>,
  phase: RuntimePhase,
) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if snapshots.borrow().phase == phase {
        return;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn force_stop_aborts_clears_queue_and_terminates_the_actor() {
  let (controller, task) = spawn_runtime(pending_kernel(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  wait_for_phase(&mut snapshots, RuntimePhase::Streaming).await;
  controller
    .execute(AgentCommand::QueueFollowUp {
      text: "must not run".into(),
    })
    .await
    .unwrap();

  controller.execute(AgentCommand::ForceStop).await.unwrap();
  wait_for_phase(&mut snapshots, RuntimePhase::FailedTerminal).await;
  assert!(snapshots.borrow().queued_follow_ups.is_empty());
  tokio::time::timeout(Duration::from_millis(250), task.join())
    .await
    .expect("force-stop must not wait for the active turn")
    .unwrap();
  assert_eq!(
    controller
      .execute(AgentCommand::StartTurn {
        text: "cannot restart".into(),
      })
      .await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}

#[tokio::test]
async fn steering_is_ready_when_start_turn_returns() {
  let (controller, task) = spawn_runtime(pending_kernel(), 4);
  controller
    .execute(AgentCommand::StartTurn {
      text: "start".into(),
    })
    .await
    .unwrap();

  controller
    .execute(AgentCommand::SteerCurrentTurn {
      text: "immediate guidance".into(),
    })
    .await
    .expect("a successful start command establishes steering readiness");

  controller.execute(AgentCommand::ForceStop).await.unwrap();
  task.join().await.unwrap();
}
