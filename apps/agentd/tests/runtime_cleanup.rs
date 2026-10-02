use std::time::Duration;

use agent_app::{AgentCommand, RuntimeCommandError, spawn_runtime};
use agent_kernel::Kernel;
use agentd::model_setup::PreparedRuntime;
use agentd::runtime_cleanup::{dispose_prepared, stop_joined_runtime};

#[tokio::test]
async fn prepared_cleanup_with_expired_grace_period_never_leaves_a_live_actor() {
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  let retained = controller.clone();

  dispose_prepared(PreparedRuntime::new(controller, runtime), Duration::ZERO).await;

  assert_eq!(
    retained
      .execute(AgentCommand::StartTurn { text: "x".into() })
      .await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}

#[tokio::test]
async fn joined_runtime_cleanup_with_expired_deadlines_aborts_actor_and_wrapper() {
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  let retained = controller.clone();
  let shutdown = runtime.shutdown_handle();
  let joined = tokio::spawn(runtime.join());
  drop(controller);

  stop_joined_runtime(shutdown, joined, Duration::ZERO).await;

  assert_eq!(
    retained
      .execute(AgentCommand::StartTurn { text: "x".into() })
      .await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}
