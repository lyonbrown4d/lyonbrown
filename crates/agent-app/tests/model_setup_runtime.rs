use agent_app::{AgentCommand, RuntimeCommandError, RuntimePhase, spawn_runtime_waiting_for_model};
use agent_kernel::Kernel;

#[tokio::test]
async fn model_setup_runtime_is_available_but_rejects_turn_commands() {
  let (controller, task) = spawn_runtime_waiting_for_model(Kernel::builder().build(), 4);

  assert_eq!(
    controller.snapshot().phase,
    RuntimePhase::ModelSetupRequired
  );
  assert_eq!(
    controller
      .execute(AgentCommand::StartTurn {
        text: "hello".into(),
      })
      .await,
    Err(RuntimeCommandError::ModelSetupRequired)
  );
  assert_eq!(
    controller
      .execute(AgentCommand::QueueFollowUp {
        text: "later".into(),
      })
      .await,
    Err(RuntimeCommandError::ModelSetupRequired)
  );

  drop(controller);
  task.join().await.unwrap();
}
