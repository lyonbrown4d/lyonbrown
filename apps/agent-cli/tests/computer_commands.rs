use agent_cli::{
  backend::Update,
  computer::{ComputerCommand, ComputerError, ComputerStatus, LocalCommand, parse_local},
  config::Config,
  model::{App, Session},
  task_command::Command,
};
use agent_computer_runtime::DesktopScope;
use clap::Parser;

#[test]
fn computer_commands_require_session_and_one_explicit_target() {
  let session = uuid::Uuid::now_v7().to_string();
  let base = ["agent-cli", "computer", "attach", "--session", &session];
  assert!(Config::try_parse_from(base).is_err());
  let mut valid = base.to_vec();
  valid.extend(["--window", "7"]);
  assert!(Config::try_parse_from(&valid).is_err());
  valid.extend(["--process-id", "19"]);
  let config = Config::try_parse_from(&valid).unwrap();
  let Some(Command::Computer {
    command: ComputerCommand::Attach(args),
  }) = config.command
  else {
    panic!("computer command")
  };
  assert_eq!(args.scope().unwrap(), DesktopScope::Window(7));
  assert_eq!(args.process_id, Some(19));
  for tail in [
    vec!["--process-id", "19"],
    vec!["--display", "2", "--process-id", "19"],
    vec!["--window", "7", "--process-id", "0"],
  ] {
    let args: Vec<_> = base.iter().copied().chain(tail).collect();
    assert!(Config::try_parse_from(&args).is_err(), "{args:?}");
  }
  assert!(Config::try_parse_from(base.iter().copied().chain(["--display", "2"])).is_ok());
  valid.extend(["--display", "2"]);
  assert!(Config::try_parse_from(valid).is_err());
  assert!(Config::try_parse_from(["agent-cli", "computer", "attach", "--display", "2"]).is_err());
  assert!(
    Config::try_parse_from([
      "agent-cli",
      "computer",
      "attach",
      "--session",
      "00000000-0000-0000-0000-000000000000",
      "--display",
      "2"
    ])
    .is_err()
  );
}

#[test]
fn local_computer_input_is_bounded_and_never_becomes_a_model_prompt() {
  assert_eq!(
    parse_local("/computer attach window 7"),
    Some(Ok(LocalCommand::Attach(DesktopScope::Window(7))))
  );
  assert_eq!(
    parse_local("/computer attach display 2"),
    Some(Ok(LocalCommand::Attach(DesktopScope::Display(2))))
  );
  for input in [
    "/computer",
    "/computer attach",
    "/computer attach all 1",
    "/computer attach display -1",
    "/computer targets extra",
  ] {
    assert_eq!(
      parse_local(input),
      Some(Err(ComputerError::InvalidCommand)),
      "{input}"
    );
  }
  assert_eq!(parse_local("normal model prompt"), None);
}

#[test]
fn runtime_status_survives_task_notices_and_session_selection_without_clearing_submit() {
  let mut app = App {
    submitting: true,
    sessions: vec![
      Session {
        id: "original-session".into(),
        title: "A".into(),
      },
      Session {
        id: "other-session".into(),
        title: "B".into(),
      },
    ],
    ..Default::default()
  };
  let status = ComputerStatus::Attached {
    session: "original-session".into(),
    scope: DesktopScope::Window(7),
  };
  agent_cli::update::apply(
    &mut app,
    Update::Computer {
      status: status.clone(),
      detail: "explicit selection".into(),
    },
  );
  assert!(app.submitting);
  app.next_session();
  assert_eq!(app.selected_session().unwrap().id, "other-session");
  agent_cli::update::apply(&mut app, Update::Notice("task event".into()));
  assert_eq!(app.computer, status);
  assert!(app.computer.summary().contains("original-session"));
  assert_eq!(app.computer_detail, "explicit selection");
}

#[cfg(not(feature = "native-computer"))]
#[test]
fn unavailable_native_build_reports_failure_without_a_daemon_or_terminal() {
  let data = tempfile::tempdir().unwrap();
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-cli"))
    .args([
      "--data-dir",
      data.path().to_str().unwrap(),
      "--endpoint",
      "http://127.0.0.1:1",
      "computer",
      "targets",
    ])
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("unavailable in this client build"));
}
