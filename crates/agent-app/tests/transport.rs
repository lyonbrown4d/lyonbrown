use agent_app::{AgentCommand, RuntimePhase, RuntimeSnapshot, channel};

#[tokio::test]
async fn transports_commands_and_publishes_snapshot_updates() {
  let (controller, mut runtime) = channel(RuntimeSnapshot::default(), 2);
  let mut snapshots = controller.subscribe();

  controller
    .send(AgentCommand::StartTurn {
      text: "hello".into(),
    })
    .await
    .unwrap();
  assert!(matches!(
    runtime.recv().await,
    Some(AgentCommand::StartTurn { text }) if text == "hello"
  ));

  let updated = RuntimeSnapshot {
    phase: RuntimePhase::Streaming,
    ..RuntimeSnapshot::default()
  };
  runtime.publish(updated.clone()).unwrap();
  snapshots.changed().await.unwrap();

  assert_eq!(*snapshots.borrow_and_update(), updated);
  assert_eq!(controller.snapshot(), updated);
}

#[tokio::test]
async fn zero_command_capacity_still_creates_a_usable_channel() {
  let (controller, mut runtime) = channel(RuntimeSnapshot::default(), 0);
  controller.send(AgentCommand::CancelTurn).await.unwrap();
  assert_eq!(runtime.recv().await, Some(AgentCommand::CancelTurn));
}
