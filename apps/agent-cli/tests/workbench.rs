use agent_cli::{
  config::{Config, ThemeChoice},
  model::{App, Connection, Session},
  theme::Theme,
  view,
};
use clap::Parser;
use ratatui::{Terminal, backend::TestBackend, style::Color};

#[test]
fn disconnect_and_failed_submission_preserve_draft() {
  let mut app = App {
    input: "A draft 🦀".into(),
    submitting: true,
    ..App::default()
  };
  agent_cli::update::apply(
    &mut app,
    agent_cli::backend::Update::Disconnected("Connection lost".into()),
  );
  assert_eq!(app.input.text(), "A draft 🦀");
  assert!(matches!(app.connection, Connection::Disconnected(_)));
  agent_cli::update::apply(
    &mut app,
    agent_cli::backend::Update::Notice("Request failed".into()),
  );
  assert!(!app.submitting);
  assert_eq!(app.input.text(), "A draft 🦀");
  agent_cli::update::apply(
    &mut app,
    agent_cli::backend::Update::Submitted("Older draft".into()),
  );
  assert_eq!(app.input.text(), "A draft 🦀");
  agent_cli::update::apply(
    &mut app,
    agent_cli::backend::Update::Submitted("A draft 🦀".into()),
  );
  assert!(app.input.is_empty());
}

#[test]
fn empty_and_changed_session_clear_stale_content() {
  let mut app = App::default();
  app.next_session();
  assert!(app.selected_session().is_none());
  app.sessions = vec![
    Session {
      id: "one".into(),
      title: "First".into(),
    },
    Session {
      id: "two".into(),
      title: "Second".into(),
    },
  ];
  app.tasks.push("Old task".into());
  app.next_session();
  assert_eq!(
    app.selected_session().map(|session| session.id.as_str()),
    Some("two")
  );
  assert!(app.tasks.is_empty());
}

#[test]
fn explicit_theme_overrides_terminal_hint() {
  assert_eq!(
    Theme::resolve(ThemeChoice::Light, Some("15;0")).accent,
    Color::Blue
  );
  assert_eq!(
    Theme::resolve(ThemeChoice::Dark, Some("0;15")).accent,
    Color::Cyan
  );
  assert_eq!(
    Theme::resolve(ThemeChoice::Auto, Some("0;15")).accent,
    Color::Blue
  );
}

#[test]
fn help_and_configuration_are_available_without_terminal() {
  let help = match Config::try_parse_from(["agent-cli", "--help"]) {
    Ok(_) => panic!("help must exit before starting terminal"),
    Err(help) => help,
  };
  assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
  assert!(help.to_string().contains("--endpoint"));
  assert!(help.to_string().contains("--token"));
  let config = Config::try_parse_from([
    "agent-cli",
    "--endpoint",
    "http://localhost:9999",
    "--theme",
    "light",
  ])
  .expect("valid config");
  assert_eq!(config.endpoint, "http://localhost:9999");
}

#[cfg(not(feature = "native-cef"))]
#[test]
fn unavailable_browser_helper_is_dispatched_before_terminal_or_clap() {
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-cli"))
    .arg("--browser-runtime")
    .output()
    .expect("run installed CLI entry");
  assert!(!output.status.success());
  let error = String::from_utf8_lossy(&output.stderr);
  assert!(
    error.contains("CEF BrowserRuntime is unavailable"),
    "{error}"
  );
}

#[test]
fn disconnected_empty_workbench_renders_at_small_sizes() {
  for (width, height) in [(120, 32), (40, 12), (10, 5)] {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
    terminal
      .draw(|frame| {
        view::render(
          frame,
          &App::default(),
          Theme::resolve(ThemeChoice::Dark, None),
        )
      })
      .expect("render workbench");
    let buffer = terminal.backend().buffer();
    let screen = buffer
      .content
      .iter()
      .map(|cell| cell.symbol())
      .collect::<String>();
    assert!(screen.contains("Lyonbrown"));
    if width >= 40 {
      assert!(screen.contains("Disconnected"));
    }
  }
}

#[test]
fn durable_run_results_and_approvals_belong_to_selected_session() {
  use agent_cli::{backend::Update, update};
  use agent_protocol::v1::{Approval, Run};
  let mut app = App {
    sessions: vec![Session {
      id: "selected".into(),
      title: "Work".into(),
    }],
    ..App::default()
  };
  update::apply(
    &mut app,
    Update::Runs(vec![
      Run {
        id: "old".into(),
        session_id: "other".into(),
        result: "Wrong session".into(),
        ..Run::default()
      },
      Run {
        id: "active".into(),
        session_id: "selected".into(),
        prompt: "Read the repo".into(),
        status: "accepted".into(),
        ..Run::default()
      },
    ]),
  );
  assert_eq!(app.active_run.as_deref(), Some("active"));
  assert_eq!(app.messages.len(), 1);
  assert_eq!(app.messages[0].content, "Read the repo");
  update::apply(
    &mut app,
    Update::Approvals {
      session: "other".into(),
      approvals: vec![Approval {
        id: "wrong".into(),
        ..Approval::default()
      }],
    },
  );
  assert!(app.approvals.is_empty());
  update::apply(
    &mut app,
    Update::Approvals {
      session: "selected".into(),
      approvals: vec![Approval {
        id: "review".into(),
        tool: "shell".into(),
        arguments_json: "{\"command\":\"ls\"}".into(),
        ..Approval::default()
      }],
    },
  );
  assert!(app.approvals[0].description.contains("shell"));
  assert!(app.approvals[0].description.contains("ls"));
  update::apply(&mut app, Update::ApprovalDecided("review".into()));
  assert!(app.approvals.is_empty());
}
