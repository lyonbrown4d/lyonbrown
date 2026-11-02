use crate::input_overlay::{complete_selected, handle_diff_key, handle_picker_key};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::{
  backend::Action,
  model::{App, Connection, ContextPanel},
};

/// Handle the same terminal events in the live loop and terminal integration tests.
/// Returns true when the client should close; daemon work remains owned by agentd.
pub fn handle(app: &mut App, event: Event, actions: &mpsc::Sender<Action>) -> anyhow::Result<bool> {
  // Dialog input is owned by transient adapters, never the chat draft/history.
  if app.provider_dialog.is_some() || app.profile_dialog.is_some() {
    return Ok(
      matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'd'))),
    );
  }
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
    Event::Paste(text) => {
      app.input.paste(&crate::view_model::safe_text(&text));
      if app.command_menu_open {
        app.refresh_command_menu();
      }
    }
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
  if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'd')) {
    return Ok(true);
  }
  // A full-screen diff and a picker own the keyboard while they are open.
  if app.diff.is_some() {
    handle_diff_key(app, key);
    return Ok(false);
  }
  if app.picker.is_some() {
    handle_picker_key(app, key, actions)?;
    return Ok(false);
  }
  if key.modifiers.contains(KeyModifiers::CONTROL) {
    app.close_command_menu();
    match key.code {
      KeyCode::Char('c' | 'd') => return Ok(true),
      KeyCode::Char('n') => send(actions, Action::NewSession)?,
      KeyCode::Char('s') => {
        crate::run_input_ui::submit(app, actions, agent_core::events::QueueKind::Steer)?
      }
      KeyCode::Char('p') => crate::history_ui::open(app),
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
        if let Some(id) = app
          .workflow
          .approvals
          .decision_id(agent_core::now_ms())
          .map(str::to_owned)
        {
          send(
            actions,
            Action::Approve {
              id,
              approved: key.code == KeyCode::Char('y'),
            },
          )?;
        }
      }
      _ => app.input.key(key),
    }
  } else {
    match key.code {
      KeyCode::Up if app.router.current() == ContextPanel::Inputs => {
        app.input_selected = app.input_selected.saturating_sub(1)
      }
      KeyCode::Down if app.router.current() == ContextPanel::Inputs => {
        app.input_selected =
          (app.input_selected + 1).min(app.input_receipts.len().saturating_sub(1))
      }
      KeyCode::Delete if app.router.current() == ContextPanel::Inputs => {
        crate::run_input_ui::cancel_selected(app, actions)?
      }
      KeyCode::Char(ch @ ('i' | 'r' | 'c' | 'a' | 'x'))
        if app.router.current() == ContextPanel::Tasks && app.input.is_empty() =>
      {
        crate::workflow_ui::request_task(app, actions, ch)?
      }
      KeyCode::Up if app.router.current() == ContextPanel::Approval => {
        app.workflow.approvals.previous()
      }
      KeyCode::Down if app.router.current() == ContextPanel::Approval => {
        app.workflow.approvals.next()
      }
      KeyCode::Char(' ') if app.router.current() == ContextPanel::Approval => {
        app.workflow.approvals.toggle_expanded()
      }
      KeyCode::Up if app.router.current() == ContextPanel::Activity => {
        app.activity.select_previous();
        crate::activity_ui::focus(app);
      }
      KeyCode::Down if app.router.current() == ContextPanel::Activity => {
        app.activity.select_next();
        crate::activity_ui::focus(app);
      }
      KeyCode::Enter if app.router.current() == ContextPanel::Activity => {
        app.activity.toggle_selected();
        crate::activity_ui::focus(app);
      }
      KeyCode::Up if app.router.current() == ContextPanel::Closed && app.input.cursor().0 == 0 => {
        if let Some(session) = app.selected_session().map(|session| session.id.clone())
          && let Some(text) = app.interaction.previous(&session, &app.input.text())
        {
          app.input.replace_text(&text);
        }
      }
      KeyCode::Down
        if app.router.current() == ContextPanel::Closed
          && app.input.cursor().0 + 1 >= app.input.text().lines().count() =>
      {
        if let Some(session) = app.selected_session().map(|session| session.id.clone())
          && let Some(text) = app.interaction.next(&session)
        {
          app.input.replace_text(&text);
        }
      }
      KeyCode::Esc if app.command_menu_open => {
        app.close_command_menu();
      }
      KeyCode::Up if app.command_menu_open => {
        app.command_selected = app.command_selected.saturating_sub(1);
      }
      KeyCode::Up if app.router.current() == ContextPanel::Sessions => {
        app.sessions_cursor = app.sessions_cursor.saturating_sub(1);
      }
      KeyCode::Down if app.command_menu_open => {
        let query = app.command_query();
        let count = crate::slash_command::filter(&query).len();
        if count > 0 {
          app.command_selected = (app.command_selected + 1).min(count - 1);
        }
      }
      KeyCode::Down if app.router.current() == ContextPanel::Sessions => {
        app.sessions_cursor = (app.sessions_cursor + 1).min(app.sessions.len().saturating_sub(1));
      }
      KeyCode::Tab if app.command_menu_open => {
        complete_selected(app);
      }
      KeyCode::Enter if app.command_menu_open => {
        let text = app.input.text();
        let token = text.split_whitespace().next().unwrap_or("/").to_owned();
        let command = token.trim_start_matches('/').to_owned();
        if text.trim() == "/" || crate::slash_command::find(&command).is_none() {
          if !complete_selected(app) {
            app.notice = format!("未知命令 {token}。输入 /help 查看可用命令。");
            app.input.clear();
            app.close_command_menu();
          }
        } else {
          app.close_command_menu();
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
      KeyCode::F(number @ 1..=7) => {
        app.close_command_menu();
        let panel = match number {
          1 => ContextPanel::Help,
          2 => ContextPanel::Sessions,
          3 => ContextPanel::Tasks,
          4 => ContextPanel::Computer,
          5 => ContextPanel::Approval,
          6 => ContextPanel::Activity,
          _ => ContextPanel::Inputs,
        };
        if number == 7 {
          crate::run_input_ui::open(app, actions)?;
        } else {
          app.router.toggle(panel);
        }
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
        app.close_command_menu();
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
        app.close_command_menu();
        app.router.navigate(ContextPanel::Computer);
        app.router.set_scroll(app.router.scroll().saturating_add(3));
      }
      KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
        app.close_command_menu();
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
        app.workflow.tasks.next();
        crate::workflow_ui::sync_task_index(app);
      }
      KeyCode::Up if app.router.current() == ContextPanel::Tasks => {
        app.workflow.tasks.previous();
        crate::workflow_ui::sync_task_index(app);
      }
      KeyCode::PageDown if app.router.current() != ContextPanel::Closed => {
        app.router.set_scroll(app.router.scroll().saturating_add(5));
      }
      KeyCode::PageUp if app.router.current() != ContextPanel::Closed => {
        app.router.set_scroll(app.router.scroll().saturating_sub(5));
      }
      KeyCode::Char('n' | 'p') if key.modifiers.contains(KeyModifiers::ALT) => {
        crate::reading_ui::next(app, key.code == KeyCode::Char('n'))
      }
      KeyCode::End if app.transcript_anchor.is_some() => {
        app.transcript_anchor = None;
        app.scroll = 0;
      }
      KeyCode::PageDown if app.transcript_anchor.is_some() => {
        app.scroll = app.scroll.saturating_add(5)
      }
      KeyCode::PageUp if app.transcript_anchor.is_some() => {
        app.scroll = app.scroll.saturating_sub(5)
      }
      KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(5),
      KeyCode::PageUp => app.scroll = app.scroll.saturating_add(5),
      KeyCode::Char('/') if app.input.is_empty() => {
        app.input.key(key);
        app.open_command_menu();
      }
      KeyCode::Char(_) | KeyCode::Backspace if app.command_menu_open => {
        app.input.key(key);
        app.refresh_command_menu();
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
