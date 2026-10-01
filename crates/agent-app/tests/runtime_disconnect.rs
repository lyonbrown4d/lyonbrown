use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentCommand, RuntimePhase, spawn_runtime};
use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use provider_mock::{MockProvider, ScriptedStep};

#[tokio::test]
async fn dropping_the_controller_cancels_active_work_and_discards_the_queue() {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::pending(),
    ScriptedStep::text("must not run"),
  ])));
  let kernel = Kernel::builder().providers(providers).build();
  let (controller, task) = spawn_runtime(kernel, 4);
  let snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "first".into(),
    })
    .await
    .unwrap();
  controller
    .execute(AgentCommand::QueueFollowUp {
      text: "second".into(),
    })
    .await
    .unwrap();

  drop(controller);
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("runtime must stop after its client disconnects")
    .unwrap();

  let snapshot = snapshots.borrow();
  assert!(snapshot.queued_follow_ups.is_empty());
  assert_eq!(snapshot.turns.len(), 1);
  assert!(matches!(snapshot.phase, RuntimePhase::Ready));
}
