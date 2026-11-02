use crate::{backend::Action, model::App};
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;
pub(crate) fn handle_diff_key(app: &mut App, key: KeyEvent) {
  if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
    app.close_diff();
    return;
  }
  let viewport = app.viewport.saturating_sub(2).max(3);
  if let Some(view) = app.diff.as_mut() {
    crate::diff::input::handle(&mut view.state, &view.document, key, viewport);
  }
}

pub(crate) fn handle_picker_key(
  app: &mut App,
  key: KeyEvent,
  actions: &mpsc::Sender<Action>,
) -> anyhow::Result<()> {
  match key.code {
    KeyCode::Esc => app.close_picker(),
    KeyCode::Up => {
      if let Some(picker) = app.picker.as_mut() {
        picker.previous();
      }
    }
    KeyCode::Down => {
      if let Some(picker) = app.picker.as_mut() {
        picker.next();
      }
    }
    KeyCode::Enter => crate::picker_command::confirm(app, actions)?,
    KeyCode::Char(ch) => {
      if let Some(picker) = app.picker.as_mut() {
        picker.filter(ch);
      }
    }
    KeyCode::Backspace => {
      if let Some(picker) = app.picker.as_mut() {
        picker.backspace();
      }
    }
    _ => {}
  }
  Ok(())
}

/// Writes the highlighted command into the composer and closes completion.
pub(crate) fn complete_selected(app: &mut App) -> bool {
  let query = app.command_query();
  let matches = crate::slash_command::filter(&query);
  let Some(name) = matches.get(app.command_selected).map(|spec| spec.name) else {
    return false;
  };
  app.input = crate::composer::Composer::from(format!("/{name} ").as_str());
  app.close_command_menu();
  true
}
