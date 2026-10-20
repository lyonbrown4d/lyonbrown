use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::{
  backend::Action,
  model::{App, Connection},
};

/// Handle the same terminal events in the live loop and terminal integration tests.
/// Returns true when the client should close; daemon work remains owned by agentd.
pub fn handle(app: &mut App, event: Event, actions: &mpsc::Sender<Action>) -> anyhow::Result<bool> {
  match event {
    Event::Key(key) if key.kind == KeyEventKind::Press => return handle_key(app, key, actions),
    Event::Paste(text) => app.input.paste(&text),
    _ => {}
  }
  Ok(false)
}

fn handle_key(
  app: &mut App,
  key: KeyEvent,
  actions: &mpsc::Sender<Action>,
) -> anyhow::Result<bool> {
  if key.modifiers.contains(KeyModifiers::CONTROL) {
    match key.code {
      KeyCode::Char('c' | 'd') => return Ok(true),
      KeyCode::Char('n') => send(actions, Action::NewSession)?,
      KeyCode::Char('r') => {
        send(actions, Action::Connect)?;
        app.connection = Connection::Connecting;
      }
      KeyCode::Char('x') => {
        if let Some(id) = &app.active_run {
          send(actions, Action::Cancel(id.clone()))?;
        }
      }
      KeyCode::Char('y' | 'e') => {
        if let Some(approval) = app.approvals.first() {
          send(
            actions,
            Action::Approve {
              id: approval.id.clone(),
              approved: key.code == KeyCode::Char('y'),
            },
          )?;
        }
      }
      _ => app.input.key(key),
    }
  } else {
    match key.code {
      KeyCode::Enter
        if key
          .modifiers
          .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
      {
        app.input.newline();
      }
      KeyCode::Enter => submit(app, actions)?,
      KeyCode::Tab => {
        app.next_session();
        if let Some(session) = app.selected_session() {
          send(actions, Action::Select(session.id.clone()))?;
        }
      }
      KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
        app.computer_scroll = app.computer_scroll.saturating_add(3);
      }
      KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
        app.computer_scroll = app.computer_scroll.saturating_sub(3);
      }
      KeyCode::PageDown => app.scroll = app.scroll.saturating_add(5),
      KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(5),
      _ => app.input.key(key),
    }
  }
  Ok(false)
}

fn submit(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  let prompt = app.input.text();
  if let Some(command) = crate::computer::parse_local(&prompt) {
    match command {
      Ok(command) => {
        send(
          actions,
          Action::Computer {
            session: app.selected_session().map(|session| session.id.clone()),
            command,
          },
        )?;
        app.input.clear();
      }
      Err(error) => app.computer_detail = error.to_string(),
    }
  } else if app.connection != Connection::Connected {
    app.notice = "Disconnected: Ctrl+R reconnects. Your draft is preserved.".into();
  } else if let Some(session) = app.selected_session() {
    if !app.submitting && !prompt.trim().is_empty() {
      send(
        actions,
        Action::Submit {
          session: session.id.clone(),
          prompt,
        },
      )?;
      app.submitting = true;
    }
  } else {
    app.notice = "Ctrl+N creates a session before sending.".into();
  }
  Ok(())
}

pub(crate) fn send(actions: &mpsc::Sender<Action>, action: Action) -> anyhow::Result<()> {
  actions
    .try_send(action)
    .map_err(|_| anyhow::anyhow!("Client request queue unavailable; reconnect before retrying"))
}
