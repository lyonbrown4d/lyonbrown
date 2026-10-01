use std::sync::Arc;

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, AvailabilityKind, ProtocolErrorKind,
  RuntimeCommandError, RuntimePhase, RuntimeSnapshot, channel,
};

#[tokio::test]
async fn app_controller_works_through_object_safe_client_port() {
  let (controller, mut runtime) = channel(RuntimeSnapshot::default(), 2);
  let client: Arc<dyn AgentClient> = Arc::new(controller);
  let mut snapshots = client.subscribe();

  let (result, command) = tokio::join!(
    client.execute(AgentCommand::StartTurn {
      text: "shared UI port".into(),
    }),
    runtime.recv()
  );
  result.unwrap();
  assert!(matches!(
    command,
    Some(AgentCommand::StartTurn { text }) if text == "shared UI port"
  ));

  runtime
    .publish(RuntimeSnapshot {
      phase: RuntimePhase::Ready,
      ..RuntimeSnapshot::default()
    })
    .unwrap();
  snapshots.changed().await.unwrap();

  assert_eq!(client.snapshot().phase, RuntimePhase::Ready);
  assert_eq!(snapshots.borrow().phase, RuntimePhase::Ready);
}

#[tokio::test]
async fn command_failure_is_exposed_as_client_error() {
  let (controller, runtime) = channel(RuntimeSnapshot::default(), 1);
  let client: Arc<dyn AgentClient> = Arc::new(controller);
  drop(runtime);

  let error = client.execute(AgentCommand::CancelTurn).await.unwrap_err();

  assert_eq!(
    error,
    AgentClientError::Command(RuntimeCommandError::RuntimeStopped)
  );
}

#[test]
fn availability_errors_have_a_stable_kind_and_human_message() {
  let error = AgentClientError::unavailable(
    AvailabilityKind::Timeout,
    "daemon did not respond before the deadline",
  );

  assert_eq!(AvailabilityKind::Timeout.code(), "timeout");
  assert_eq!(AvailabilityKind::Timeout.to_string(), "timeout");
  assert_eq!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::Timeout,
      message: "daemon did not respond before the deadline".into(),
    }
  );
  assert_eq!(
    error.to_string(),
    "agent client unavailable (timeout): daemon did not respond before the deadline"
  );
}

#[test]
fn protocol_errors_have_a_stable_kind_and_human_message() {
  let error = AgentClientError::protocol(
    ProtocolErrorKind::IncompatibleVersion,
    "server requires protocol v2",
  );

  assert_eq!(
    ProtocolErrorKind::IncompatibleVersion.code(),
    "incompatible_version"
  );
  assert_eq!(
    ProtocolErrorKind::IncompatibleVersion.to_string(),
    "incompatible_version"
  );
  assert_eq!(
    error,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::IncompatibleVersion,
      message: "server requires protocol v2".into(),
    }
  );
  assert_eq!(
    error.to_string(),
    "agent protocol failed (incompatible_version): server requires protocol v2"
  );
}
