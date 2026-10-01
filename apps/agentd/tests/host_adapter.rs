use agent_app::{AgentCommand, RuntimePhase, RuntimeSnapshot, channel};
use agentd::{AppControllerHost, RuntimeHost};

#[tokio::test]
async fn app_controller_adapter_forwards_commands_and_versions_snapshots() {
  let (controller, mut runtime) = channel(RuntimeSnapshot::default(), 2);
  let host = AppControllerHost::new(controller);
  let mut snapshots = host.subscribe();

  let (result, command) = tokio::join!(
    host.execute(AgentCommand::StartTurn {
      text: "through daemon".into(),
    }),
    runtime.recv()
  );
  result.unwrap();
  assert!(matches!(
    command,
    Some(AgentCommand::StartTurn { text }) if text == "through daemon"
  ));

  runtime
    .publish(RuntimeSnapshot {
      phase: RuntimePhase::Ready,
      ..RuntimeSnapshot::default()
    })
    .unwrap();
  snapshots.changed().await.unwrap();

  assert_eq!(snapshots.borrow().revision, 2);
  assert_eq!(snapshots.borrow().snapshot.phase, RuntimePhase::Ready);
}
