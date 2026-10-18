use agent_cli::{
  config::Config,
  task_command::{Command, TaskCommand},
  task_management::dispatch_envelope,
};
use agent_protocol::v1;
use clap::Parser;

fn dispatch_args() -> Vec<&'static str> {
  vec![
    "agent-cli",
    "task",
    "dispatch",
    "--node-id",
    "worker-1",
    "--node-endpoint",
    "https://worker.example:7448",
    "--session",
    "session-1",
    "--parent-run",
    "run-1",
    "--task-id",
    "01900000-0000-7000-8000-000000000001",
    "--attempt-id",
    "01900000-0000-7000-8000-000000000002",
    "--idempotency-key",
    "dispatch-1",
    "--instructions",
    "Read workspace",
    "--capability",
    "workspace.read",
  ]
}

#[test]
fn task_arguments_require_stable_identity_and_bounded_authority() {
  let config = Config::try_parse_from(dispatch_args()).expect("valid bounded dispatch");
  assert!(matches!(
    config.command,
    Some(Command::Task {
      command: TaskCommand::Dispatch(_)
    })
  ));
  for (flag, invalid) in [
    ("--task-id", "fresh-id"),
    ("--capability", "shell"),
    ("--deadline-seconds", "86401"),
  ] {
    let mut args = dispatch_args();
    if let Some(index) = args.iter().position(|arg| *arg == flag) {
      args[index + 1] = invalid;
    } else {
      args.extend([flag, invalid]);
    }
    assert!(Config::try_parse_from(args).is_err(), "reject {flag}");
  }
  let mut args = dispatch_args();
  let index = args
    .iter()
    .position(|arg| *arg == "--idempotency-key")
    .expect("flag");
  args.drain(index..=index + 1);
  assert!(Config::try_parse_from(args).is_err());
  assert!(
    Config::try_parse_from([
      "agent-cli",
      "task",
      "list",
      "--session",
      "session-1",
      "--page-size",
      "501"
    ])
    .is_err()
  );
  assert!(
    Config::try_parse_from([
      "agent-cli",
      "task",
      "accept",
      "task-1",
      "--node-id",
      "worker-1",
      "--node-endpoint",
      "https://worker.example"
    ])
    .is_err()
  );
}

#[test]
fn dispatch_uses_current_master_session_and_requires_its_active_parent() {
  let Some(Command::Task {
    command: TaskCommand::Dispatch(mut args),
  }) = Config::try_parse_from(dispatch_args())
    .expect("dispatch arguments")
    .command
  else {
    panic!("dispatch expected")
  };
  let session = v1::Session {
    id: "session-1".into(),
    owner_node_id: "master-1".into(),
    owner_epoch: 7,
    workspace: "/workspace".into(),
    ..Default::default()
  };
  let mut parent = v1::Run {
    id: "run-1".into(),
    session_id: session.id.clone(),
    status: "running".into(),
    ..Default::default()
  };
  let node = v1::Node {
    id: "master-1".into(),
    ..Default::default()
  };
  let envelope = dispatch_envelope(&args, &session, &parent, &node, 1000).expect("valid task");
  assert_eq!(envelope.owner_epoch, 7);
  assert_eq!(envelope.workspace, session.workspace);
  assert_eq!(envelope.task_id, args.task_id);
  assert_eq!(envelope.deadline_ms, 3_601_000);
  parent.session_id = "another-session".into();
  assert!(dispatch_envelope(&args, &session, &parent, &node, 1000).is_err());
  parent.session_id = session.id.clone();
  parent.status = "completed".into();
  assert!(dispatch_envelope(&args, &session, &parent, &node, 1000).is_err());
  parent.status = "running".into();
  args.instructions = "x".repeat(64 * 1024);
  assert!(dispatch_envelope(&args, &session, &parent, &node, 1000).is_err());
  args.instructions = " ".into();
  assert!(dispatch_envelope(&args, &session, &parent, &node, 1000).is_err());
}
