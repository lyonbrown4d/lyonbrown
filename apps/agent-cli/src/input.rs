use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::{
  backend::Action,
  model::{App, Connection, ContextPanel},
};

/// Handle the same terminal events in the live loop and terminal integration tests.
/// Returns true when the client should close; daemon work remains owned by agentd.
pub fn handle(app: &mut App, event: Event, actions: &mpsc::Sender<Action>) -> anyhow::Result<bool> {
  if app.connection == Connection::Connecting
    || (app.connection == Connection::Connected && !app.state_loaded)
  {
    if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'd')))
    {
      return Ok(true);
    }
    app.notice = "Restoring client state; input is temporarily paused.".into();
    return Ok(false);
  }
  match event {
    Event::Key(key) if key.kind == KeyEventKind::Press => {
      let close = handle_key(app, key, actions)?;
      app.save_current_draft();
      return Ok(close);
    }
    Event::Paste(text) => app.input.paste(&crate::view_model::safe_text(&text)),
    _ => {}
  }
  app.save_current_draft();
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
        if let Some(node_id) = app.persistence_node_id.clone() {
          let state = app.client_state();
          send(actions, Action::SaveState { node_id, state })?;
        }
        app.state_loaded = false;
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
      KeyCode::Esc if app.router.current() == ContextPanel::Commands => {
        app.router.back();
      }
      KeyCode::Up if app.router.current() == ContextPanel::Commands => {
        app.command_selected = app.command_selected.saturating_sub(1);
      }
      KeyCode::Up if app.router.current() == ContextPanel::Sessions => {
        app.sessions_cursor = app.sessions_cursor.saturating_sub(1);
      }
      KeyCode::Down if app.router.current() == ContextPanel::Commands => {
        let query = app.input.text();
        let query = query.split_whitespace().next().unwrap_or("/");
        let count = crate::slash_command::filter(query).len();
        if count > 0 {
          app.command_selected = (app.command_selected + 1).min(count - 1);
        }
      }
      KeyCode::Down if app.router.current() == ContextPanel::Sessions => {
        app.sessions_cursor = (app.sessions_cursor + 1).min(app.sessions.len().saturating_sub(1));
      }
      KeyCode::Tab if app.router.current() == ContextPanel::Commands => {
        let query = app.input.text();
        let query = query.split_whitespace().next().unwrap_or("/");
        if let Some(command) = crate::slash_command::filter(query).get(app.command_selected) {
          app.input = crate::composer::Composer::from(format!("/{} ", command.name).as_str());
          app.router.back();
        }
      }
      KeyCode::Enter if app.router.current() == ContextPanel::Commands => {
        let text = app.input.text();
        let token = text.split_whitespace().next().unwrap_or("/");
        let command = token.trim_start_matches('/');
        if text.trim() == "/" || crate::slash_command::find(command).is_none() {
          let query = text.split_whitespace().next().unwrap_or("/");
          if let Some(command) = crate::slash_command::filter(query).get(app.command_selected) {
            app.input = crate::composer::Composer::from(format!("/{} ", command.name).as_str());
            app.router.back();
          } else {
            app.notice = format!("未知命令 {token}。输入 /help 查看可用命令。");
            app.input.clear();
            app.router.back();
          }
        } else {
          app.router.back();
          crate::local_command::submit(app, actions)?;
        }
      }
      KeyCode::Enter if app.router.current() == ContextPanel::Sessions => {
        if let Some(session) = app
          .sessions
          .get(app.sessions_cursor)
          .map(|session| session.id.clone())
        {
          app.select_session(app.sessions_cursor);
          send(
            actions,
            Action::Select {
              session,
              generation: app.generation,
            },
          )?;
          app.router.back();
        }
      }
      KeyCode::F(number @ 1..=5) => {
        let panel = match number {
          1 => ContextPanel::Help,
          2 => ContextPanel::Sessions,
          3 => ContextPanel::Tasks,
          4 => ContextPanel::Computer,
          _ => ContextPanel::Approval,
        };
        app.router.toggle(panel);
        if number == 2 && app.router.current() == ContextPanel::Sessions {
          app.sessions_cursor = app.selected;
        }
        if number == 3 && app.router.current() == ContextPanel::Tasks {
          crate::local_command::request_task_page(app, actions, "")?;
        }
      }
      KeyCode::Esc => app.router.back(),
      KeyCode::Enter
        if key
          .modifiers
          .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
      {
        app.input.newline();
      }
      KeyCode::Enter
        if app.router.current() == ContextPanel::Tasks
          && app.input.is_empty()
          && !app.task_rows.is_empty() =>
      {
        if let Some(task_id) = app
          .task_rows
          .get(app.task_selected)
          .and_then(|row| row.get("envelope"))
          .and_then(|envelope| envelope.get("task_id"))
          .and_then(serde_json::Value::as_str)
        {
          send(
            actions,
            Action::Task {
              session: app.selected_session().map(|session| session.id.clone()),
              generation: app.generation,
              source: None,
              append: false,
              command: crate::task_command::TaskCommand::Get {
                task_id: task_id.into(),
              },
            },
          )?;
        }
      }
      KeyCode::Enter => crate::local_command::submit(app, actions)?,
      KeyCode::Tab => {
        app.next_session();
        app.sessions_cursor = app.selected;
        if let Some(session) = app.selected_session() {
          send(
            actions,
            Action::Select {
              session: session.id.clone(),
              generation: app.generation,
            },
          )?;
        }
        if app.router.current() == ContextPanel::Tasks {
          crate::local_command::request_task_page(app, actions, "")?;
        }
      }
      KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
        app.router.navigate(ContextPanel::Computer);
        app.router.set_scroll(app.router.scroll().saturating_add(3));
      }
      KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
        app.router.navigate(ContextPanel::Computer);
        app.router.set_scroll(app.router.scroll().saturating_sub(3));
      }
      KeyCode::PageDown
        if app.router.current() == ContextPanel::Tasks && !app.task_cursor.is_empty() =>
      {
        let cursor = app.task_cursor.clone();
        crate::local_command::request_task_page(app, actions, &cursor)?;
      }
      KeyCode::Down if app.router.current() == ContextPanel::Tasks && !app.task_rows.is_empty() => {
        app.task_selected = (app.task_selected + 1).min(app.task_rows.len().saturating_sub(1));
      }
      KeyCode::Up if app.router.current() == ContextPanel::Tasks => {
        app.task_selected = app.task_selected.saturating_sub(1);
      }
      KeyCode::PageDown if app.router.current() != ContextPanel::Closed => {
        app.router.set_scroll(app.router.scroll().saturating_add(5));
      }
      KeyCode::PageUp if app.router.current() != ContextPanel::Closed => {
        app.router.set_scroll(app.router.scroll().saturating_sub(5));
      }
      KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(5),
      KeyCode::PageUp => app.scroll = app.scroll.saturating_add(5),
      KeyCode::Char('/') if app.input.is_empty() => {
        app.input.key(key);
        app.router.navigate(ContextPanel::Commands);
        app.command_selected = 0;
      }
      KeyCode::Char(_) | KeyCode::Backspace if app.router.current() == ContextPanel::Commands => {
        app.input.key(key);
        let text = app.input.text();
        if text.split_whitespace().count() > 1 {
          app.router.back();
        } else {
          app.command_selected = 0;
        }
      }
      _ => app.input.key(key),
    }
  }
  Ok(false)
}

pub(crate) fn send(actions: &mpsc::Sender<Action>, action: Action) -> anyhow::Result<()> {
  actions
    .try_send(action)
    .map_err(|_| anyhow::anyhow!("Client request queue unavailable; reconnect before retrying"))
}
