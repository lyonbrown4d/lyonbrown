use agent_cli::{
  backend::Action,
  input,
  layout::{AppLayout, BodyLayout},
  model::{App, Connection, ContextPanel, Session},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tokio::sync::mpsc;

fn key(app: &mut App, actions: &mpsc::Sender<Action>, code: KeyCode, modifiers: KeyModifiers) {
  assert!(
    !input::handle(app, Event::Key(KeyEvent::new(code, modifiers)), actions).expect("client key")
  );
}

#[test]
fn back_restores_panel_scroll_and_palette_origin_without_dispatching_daemon_work() {
  let mut app = App {
    connection: Connection::Connected,
    ..App::default()
  };
  let (actions, mut requests) = mpsc::channel(8);
  key(&mut app, &actions, KeyCode::F(4), KeyModifiers::NONE);
  key(&mut app, &actions, KeyCode::PageDown, KeyModifiers::ALT);
  assert_eq!(app.router.scroll(), 3);
  key(&mut app, &actions, KeyCode::F(1), KeyModifiers::NONE);
  assert_eq!(app.router.current(), ContextPanel::Help);
  assert_eq!(app.router.scroll(), 0);
  key(&mut app, &actions, KeyCode::Char('/'), KeyModifiers::NONE);
  assert!(
    app.command_menu_open,
    "slash opens completion over the composer"
  );
  assert_eq!(
    app.router.current(),
    ContextPanel::Help,
    "completion never replaces the open panel"
  );
  key(&mut app, &actions, KeyCode::Esc, KeyModifiers::NONE);
  assert!(!app.command_menu_open);
  assert_eq!(app.router.current(), ContextPanel::Help);
  key(&mut app, &actions, KeyCode::Esc, KeyModifiers::NONE);
  assert_eq!(app.router.current(), ContextPanel::Computer);
  assert_eq!(app.router.scroll(), 3);
  key(&mut app, &actions, KeyCode::F(4), KeyModifiers::NONE);
  assert_eq!(app.router.current(), ContextPanel::Closed);
  assert_eq!(
    app.input.text(),
    "/",
    "navigation preserves composer content"
  );
  assert!(
    requests.try_recv().is_err(),
    "navigation grants no daemon authority"
  );
}

#[test]
fn session_change_discards_transient_route_history_but_preserves_the_previous_draft() {
  let mut app = App {
    sessions: vec![
      Session {
        id: "a".into(),
        title: "A".into(),
      },
      Session {
        id: "b".into(),
        title: "B".into(),
      },
    ],
    input: agent_cli::composer::Composer::from("unfinished"),
    ..App::default()
  };
  app.router.navigate(ContextPanel::Help);
  app.router.set_scroll(19);
  assert!(app.select_session(1));
  assert_eq!(
    app.router.current(),
    ContextPanel::Help,
    "the open page survives a session switch"
  );
  app.router.back();
  assert_eq!(
    app.router.current(),
    ContextPanel::Closed,
    "no stale return stack from session A"
  );
  assert_eq!(app.router.scroll(), 0);
  assert!(app.input.is_empty());
  assert_eq!(app.drafts.get("a").map(String::as_str), Some("unfinished"));
  assert!(app.select_session(0));
  assert_eq!(app.input.text(), "unfinished");
}

#[test]
fn delayed_resource_result_lands_in_the_transcript_without_moving_the_user() {
  use agent_cli::{backend::Update, update};
  let mut app = App {
    connection: Connection::Connected,
    sessions: vec![Session {
      id: "s1".into(),
      title: "S".into(),
    }],
    input: "/status".into(),
    ..App::default()
  };
  let (actions, mut requests) = mpsc::channel(8);
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  let Action::Resource {
    session,
    generation,
    navigation_revision,
    source,
    ..
  } = requests.try_recv().expect("resource request")
  else {
    panic!("expected resource request");
  };
  key(&mut app, &actions, KeyCode::F(1), KeyModifiers::NONE);
  assert_eq!(app.router.current(), ContextPanel::Help);
  app.router.set_scroll(8);
  update::apply(
    &mut app,
    Update::ResourceDetail {
      session,
      generation,
      navigation_revision,
      source,
      title: "Providers".into(),
      detail: "older result".into(),
    },
  );
  assert_eq!(
    app.router.current(),
    ContextPanel::Help,
    "command output never navigates"
  );
  assert_eq!(app.router.scroll(), 8);
  let last = app.messages.last().expect("transcript message");
  assert_eq!(last.role, "Local");
  assert!(last.content.contains("older result"));
}

#[test]
fn background_computer_status_preserves_the_visible_help_scroll() {
  use agent_cli::{backend::Update, update};
  let mut app = App::default();
  app.router.navigate(ContextPanel::Computer);
  app.router.set_scroll(7);
  app.router.navigate(ContextPanel::Help);
  app.router.set_scroll(12);
  update::apply(
    &mut app,
    Update::Computer {
      status: agent_cli::computer::ComputerStatus::Detached,
      detail: "updated status".into(),
    },
  );
  assert_eq!(app.router.current(), ContextPanel::Help);
  assert_eq!(app.router.scroll(), 12);
  app.router.back();
  assert_eq!(app.router.current(), ContextPanel::Computer);
  assert_eq!(
    app.router.scroll(),
    7,
    "inactive route context remains intact"
  );
}

#[test]
fn restored_panel_escape_returns_to_chat_and_root_escape_revokes_pending_presentation() {
  use agent_cli::{backend::Update, update};
  let mut app = App {
    connection: Connection::Connected,
    router: ContextPanel::Help.into(),
    sessions: vec![Session {
      id: "s1".into(),
      title: "S".into(),
    }],
    input: "/status".into(),
    ..App::default()
  };
  let (actions, mut requests) = mpsc::channel(8);
  key(&mut app, &actions, KeyCode::Esc, KeyModifiers::NONE);
  assert_eq!(app.router.current(), ContextPanel::Closed);
  key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
  let Action::Resource {
    session,
    generation,
    navigation_revision,
    source,
    ..
  } = requests.try_recv().expect("resource request")
  else {
    panic!("expected resource request");
  };
  key(&mut app, &actions, KeyCode::Esc, KeyModifiers::NONE);
  update::apply(
    &mut app,
    Update::ResourceDetail {
      session,
      generation,
      navigation_revision,
      source,
      title: "Providers".into(),
      detail: "result".into(),
    },
  );
  assert_eq!(
    app.router.current(),
    ContextPanel::Closed,
    "late result cannot reopen after root Escape"
  );
}

#[test]
fn responsive_regions_stay_inside_tiny_and_wide_terminal_bounds() {
  for (width, height) in [(0, 0), (1, 1), (20, 4), (69, 15), (70, 16), (120, 40)] {
    let terminal = Rect::new(3, 5, width, height);
    let layout = AppLayout::new(terminal, Some(usize::MAX));
    for region in [
      layout.header,
      layout.body,
      layout.approval,
      layout.composer,
      layout.footer,
      layout.palette(usize::MAX).area,
    ] {
      assert!(region.x >= terminal.x && region.y >= terminal.y);
      assert!(region.right() <= terminal.right() && region.bottom() <= terminal.bottom());
    }
    let body = BodyLayout::new(layout.body, true);
    assert_eq!(body.transcript.is_some(), layout.body.width >= 95);
    assert!(body.context.is_some());
    let closed = BodyLayout::new(layout.body, false);
    assert_eq!(closed.transcript, Some(layout.body));
    assert_eq!(closed.context, None);
  }
}

#[test]
fn queued_resource_commands_append_in_order_without_touching_newer_typing() {
  use agent_cli::{backend::Update, update};
  for initial in [ContextPanel::Closed, ContextPanel::Tasks] {
    let mut app = App {
      connection: Connection::Connected,
      router: initial.into(),
      sessions: vec![Session {
        id: "s1".into(),
        title: "S".into(),
      }],
      ..App::default()
    };
    let (actions, mut requests) = mpsc::channel(8);
    let mut replies = Vec::new();
    for detail in ["older result", "latest result"] {
      app.input = "/status".into();
      key(&mut app, &actions, KeyCode::Enter, KeyModifiers::NONE);
      let Action::Resource {
        session,
        generation,
        navigation_revision,
        source,
        ..
      } = requests.try_recv().expect("queued request")
      else {
        panic!("expected resource request");
      };
      assert!(
        app.input.is_empty(),
        "submission clears the composer immediately"
      );
      replies.push(Update::ResourceDetail {
        session,
        generation,
        navigation_revision,
        source,
        title: "Providers".into(),
        detail: detail.into(),
      });
    }
    update::apply(&mut app, replies.remove(0));
    assert_eq!(app.router.current(), initial);
    assert!(
      app
        .messages
        .last()
        .is_some_and(|message| message.content.contains("older result"))
    );
    app.input = "retain text typed while waiting".into();
    update::apply(&mut app, replies.remove(0));
    assert_eq!(app.router.current(), initial, "results never move the user");
    assert!(
      app
        .messages
        .last()
        .is_some_and(|message| message.content.contains("latest result"))
    );
    assert_eq!(app.input.text(), "retain text typed while waiting");
  }
}
