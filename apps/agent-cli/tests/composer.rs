use agent_cli::{
  backend::{Action, Update},
  config::ThemeChoice,
  input,
  model::{App, Connection, Session},
  theme::Theme,
  update, view,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};
use tokio::sync::mpsc;

fn key(app: &mut App, actions: &mpsc::Sender<Action>, code: KeyCode, modifiers: KeyModifiers) {
  assert!(!input::handle(app, Event::Key(KeyEvent::new(code, modifiers)), actions).unwrap());
}

fn paste(app: &mut App, actions: &mpsc::Sender<Action>, text: &str) {
  input::handle(app, Event::Paste(text.into()), actions).unwrap();
}

fn connected() -> App {
  App {
    connection: Connection::Connected,
    sessions: vec![Session {
      id: "session".into(),
      title: "Work".into(),
    }],
    ..App::default()
  }
}

fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
  let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
  terminal
    .draw(|frame| view::render(frame, app, Theme::resolve(ThemeChoice::Dark, None)))
    .unwrap();
  terminal
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
  terminal
    .backend()
    .buffer()
    .content
    .iter()
    .map(|cell| cell.symbol())
    .collect()
}

#[test]
fn terminal_events_edit_at_unicode_cursor_and_render_that_cursor() {
  let (actions, mut requests) = mpsc::channel(8);
  let mut app = connected();
  paste(&mut app, &actions, "你🦀");
  key(&mut app, &actions, KeyCode::Left, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Char('好'), KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Delete, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Home, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Right, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Char('🦀'), KeyModifiers::NONE);
  assert_eq!(app.input.text(), "你🦀好");
  assert_eq!(app.input.cursor(), (0, 2));
  let terminal = render(&app, 120, 32);
  assert!(screen(&terminal).replace(' ', "").contains("你🦀好"));
  assert!(
    terminal
      .backend()
      .buffer()
      .content
      .iter()
      .any(|cell| { cell.symbol() == "好" && cell.modifier.contains(Modifier::REVERSED) }),
    "the widget cursor must mark the insertion position, not the draft end"
  );
  key(&mut app, &actions, KeyCode::Backspace, KeyModifiers::NONE);
  assert_eq!(app.input.text(), "你好");
  assert!(
    requests.try_recv().is_err(),
    "editing must not issue daemon actions"
  );
}

#[test]
fn multiline_paste_and_newlines_submit_once_and_keep_newer_edits() {
  let (actions, mut requests) = mpsc::channel(8);
  let mut app = connected();
  paste(&mut app, &actions, "你好 🦀\r\nsecond line");
  key(&mut app, &actions, KeyCode::Home, KeyModifiers::NONE);
  paste(&mut app, &actions, "middle ");
  key(&mut app, &actions, KeyCode::Up, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Home, KeyModifiers::NONE);
  paste(&mut app, &actions, "first ");
  key(&mut app, &actions, KeyCode::Down, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::End, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::SHIFT);
  paste(&mut app, &actions, "third");
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::ALT);
  paste(&mut app, &actions, "last");
  let prompt = "first 你好 🦀\nmiddle second line\nthird\nlast";
  assert_eq!(app.input.text(), prompt);
  let terminal = render(&app, 40, 12);
  assert!(
    screen(&terminal).contains("last"),
    "compact composer follows the active line"
  );
  assert!(
    requests.try_recv().is_err(),
    "paste and modified Enter must not submit"
  );
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  assert!(app.submitting);
  match requests.try_recv().unwrap() {
    Action::Submit {
      session,
      prompt: submitted,
      ..
    } => {
      assert_eq!(session, "session");
      assert_eq!(submitted, prompt);
    }
    _ => panic!("expected submitted protocol prompt"),
  }
  assert!(
    requests.try_recv().is_err(),
    "Enter while awaiting acknowledgement cannot duplicate submission"
  );
  assert_eq!(
    app.input.text(),
    prompt,
    "unacknowledged text remains a draft"
  );
  paste(&mut app, &actions, " edit");
  update::apply(
    &mut app,
    Update::Submitted {
      session: "session".into(),
      prompt: prompt.into(),
      generation: 0,
    },
  );
  assert!(!app.submitting);
  assert_eq!(app.input.text(), format!("{prompt} edit"));
  let edited = app.input.text();
  update::apply(
    &mut app,
    Update::Submitted {
      session: "session".into(),
      prompt: edited,
      generation: 0,
    },
  );
  assert!(app.input.is_empty());
}

#[test]
fn disconnect_and_queue_failure_preserve_editable_unacknowledged_draft() {
  let (actions, mut requests) = mpsc::channel(8);
  let mut app = connected();
  paste(&mut app, &actions, "draft 🦀");
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  assert!(matches!(
    requests.try_recv().unwrap(),
    Action::Submit { .. }
  ));
  update::apply(&mut app, Update::Disconnected("connection lost".into()));
  key(&mut app, &actions, KeyCode::Left, KeyModifiers::NONE);
  paste(&mut app, &actions, "saved ");
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  assert_eq!(app.input.text(), "draft saved 🦀");
  assert!(app.notice.contains("draft is preserved"));
  update::apply(
    &mut app,
    Update::Connected(vec![agent_protocol::v1::Session {
      id: "session".into(),
      ..Default::default()
    }]),
  );
  assert!(
    requests.try_recv().is_err(),
    "reconnection does not replay a prompt"
  );
  assert!(screen(&render(&app, 120, 32)).contains("draft saved 🦀"));
  update::apply(
    &mut app,
    Update::Notice("request failed; outcome unknown".into()),
  );
  drop(requests);
  assert!(
    input::handle(
      &mut app,
      Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
      &actions
    )
    .is_err()
  );
  assert!(
    !app.submitting,
    "a closed request queue cannot leave submission pending"
  );
  assert_eq!(app.input.text(), "draft saved 🦀");
}

#[test]
fn application_shortcuts_take_precedence_over_editor_bindings() {
  let (actions, mut requests) = mpsc::channel(8);
  let mut app = connected();
  app.sessions.push(Session {
    id: "next".into(),
    title: "Next".into(),
  });
  paste(&mut app, &actions, "keep draft");
  key(&mut app, &actions, KeyCode::Tab, KeyModifiers::NONE);
  assert!(
    matches!(requests.try_recv().unwrap(), Action::Select { session: id, .. } if id == "next")
  );
  assert!(
    app.input.is_empty(),
    "switching sessions loads that session's draft"
  );
  assert_eq!(
    app.drafts.get("session").map(String::as_str),
    Some("keep draft")
  );
  key(
    &mut app,
    &actions,
    KeyCode::Char('n'),
    KeyModifiers::CONTROL,
  );
  assert!(matches!(requests.try_recv().unwrap(), Action::NewSession));
  app.input.clear();
  paste(&mut app, &actions, "/computer status");
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  assert!(
    matches!(requests.try_recv().unwrap(), Action::Computer { session: Some(id), .. } if id == "next")
  );
  assert!(app.input.is_empty());
  let release = KeyEvent::new_with_kind(
    KeyCode::Char('x'),
    KeyModifiers::NONE,
    KeyEventKind::Release,
  );
  input::handle(&mut app, Event::Key(release), &actions).unwrap();
  assert!(app.input.is_empty());
  assert!(
    input::handle(
      &mut app,
      Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
      &actions
    )
    .unwrap()
  );
  assert!(requests.try_recv().is_err());
}

#[test]
fn context_shortcuts_keep_editor_draft_and_existing_daemon_authority() {
  use agent_cli::model::{Approval, ContextPanel};
  let (actions, mut requests) = mpsc::channel(8);
  let mut app = connected();
  paste(&mut app, &actions, "keep draft");
  for (number, panel) in [
    (1, ContextPanel::Help),
    (2, ContextPanel::Sessions),
    (3, ContextPanel::Tasks),
    (4, ContextPanel::Computer),
    (5, ContextPanel::Approval),
  ] {
    key(&mut app, &actions, KeyCode::F(number), KeyModifiers::NONE);
    assert_eq!(app.router.current(), panel);
    assert_eq!(app.input.text(), "keep draft");
    if number == 3 {
      assert!(matches!(requests.try_recv(), Ok(Action::Task { .. })));
    } else {
      assert!(requests.try_recv().is_err());
    }
    key(&mut app, &actions, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.router.current(), ContextPanel::Closed);
  }
  app.approvals.push(Approval {
    id: "pending".into(),
    description: "write_file".into(),
  });
  key(&mut app, &actions, KeyCode::Char('y'), KeyModifiers::NONE);
  assert!(
    requests.try_recv().is_err(),
    "plain y must not grant approval"
  );
  key(
    &mut app,
    &actions,
    KeyCode::Char('y'),
    KeyModifiers::CONTROL,
  );
  assert!(
    matches!(requests.try_recv().unwrap(), Action::Approve { id, approved: true } if id == "pending")
  );
  key(&mut app, &actions, KeyCode::PageUp, KeyModifiers::NONE);
  assert!(app.scroll > 0);
  key(&mut app, &actions, KeyCode::PageDown, KeyModifiers::NONE);
  assert_eq!(app.scroll, 0);
}
