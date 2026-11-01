//! Keyboard handling for the diff view. Only view state changes here.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
  model::{DiffDocument, DiffMode, DiffViewState},
  render::total_rows,
};

/// Columns moved by `h` and `l`.
const HORIZONTAL_STEP: u16 = 4;

/// Returns true when the key was consumed by the diff view.
pub fn handle(
  state: &mut DiffViewState,
  document: &DiffDocument,
  key: KeyEvent,
  viewport: u16,
) -> bool {
  let half = i32::from((viewport / 2).max(1));
  let limit = max_scroll(document, state.mode, viewport);
  let control = key.modifiers.contains(KeyModifiers::CONTROL);
  match key.code {
    KeyCode::Char('n') if control => scroll_by(state, limit, 1),
    KeyCode::Char('p') if control => scroll_by(state, limit, -1),
    KeyCode::Char('d') if control => scroll_by(state, limit, half),
    KeyCode::Char('u') if control => scroll_by(state, limit, -half),
    KeyCode::PageDown => scroll_by(state, limit, half),
    KeyCode::PageUp => scroll_by(state, limit, -half),
    KeyCode::Char('j') | KeyCode::Down => scroll_by(state, limit, 1),
    KeyCode::Char('k') | KeyCode::Up => scroll_by(state, limit, -1),
    KeyCode::Char('g') | KeyCode::Home => {
      state.scroll = 0;
      true
    }
    KeyCode::Char('G') | KeyCode::End => {
      state.scroll = limit;
      true
    }
    KeyCode::Tab => {
      toggle_mode(state);
      state.scroll = state.scroll.min(max_scroll(document, state.mode, viewport));
      true
    }
    KeyCode::Char('l') | KeyCode::Right => {
      state.h_scroll = state.h_scroll.saturating_add(HORIZONTAL_STEP);
      true
    }
    KeyCode::Char('h') | KeyCode::Left => {
      state.h_scroll = state.h_scroll.saturating_sub(HORIZONTAL_STEP);
      true
    }
    KeyCode::Char('n') | KeyCode::Char(']') => step_file(state, document, true),
    KeyCode::Char('p') | KeyCode::Char('[') => step_file(state, document, false),
    _ => false,
  }
}

/// Cycling mode is shared with the footer hint and tests.
pub fn toggle_mode(state: &mut DiffViewState) {
  state.mode = match state.mode {
    DiffMode::Unified => DiffMode::SideBySide,
    DiffMode::SideBySide => DiffMode::Unified,
  };
  state.h_scroll = 0;
}

/// Largest top offset that still shows content for the given viewport.
fn max_scroll(document: &DiffDocument, mode: DiffMode, viewport: u16) -> u16 {
  let rows = total_rows(document, mode);
  u16::try_from(rows.saturating_sub(usize::from(viewport))).unwrap_or(u16::MAX)
}

/// Moves the top offset by `delta` rows, never leaving `[0, limit]`. Returns true so the key
/// arms stay a single expression.
fn scroll_by(state: &mut DiffViewState, limit: u16, delta: i32) -> bool {
  let step = u16::try_from(delta.unsigned_abs()).unwrap_or(u16::MAX);
  let next = if delta >= 0 {
    state.scroll.saturating_add(step)
  } else {
    state.scroll.saturating_sub(step)
  };
  state.scroll = next.min(limit);
  true
}

/// Moves the selected file and returns to the top of it.
fn step_file(state: &mut DiffViewState, document: &DiffDocument, forward: bool) -> bool {
  state.scroll = 0;
  let count = document.files.len();
  state.file = if count == 0 {
    0
  } else {
    let current = state.file.min(count - 1);
    if forward {
      (current + 1).min(count - 1)
    } else {
      current.saturating_sub(1)
    }
  };
  true
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::diff::model::{DiffLine, FileDiff, Hunk, LineKind};

  fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
  }

  fn control(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
  }

  /// `files` files with `lines` context lines each, so row counts are predictable.
  fn document(files: usize, lines: usize) -> DiffDocument {
    DiffDocument {
      files: (0..files)
        .map(|index| FileDiff {
          old_path: format!("src/{index}.rs"),
          new_path: format!("src/{index}.rs"),
          hunks: vec![Hunk {
            header: "@@ -1,1 +1,1 @@".into(),
            lines: (0..lines)
              .map(|_| DiffLine {
                kind: LineKind::Context,
                text: "line".into(),
                ..DiffLine::default()
              })
              .collect(),
            ..Hunk::default()
          }],
        })
        .collect(),
    }
  }

  #[test]
  fn vertical_scrolling_stops_at_the_last_page() {
    let document = document(1, 20);
    let mut state = DiffViewState::default();
    for _ in 0..50 {
      assert!(handle(&mut state, &document, key(KeyCode::Char('j')), 10));
    }
    assert_eq!(state.scroll, 12);
    assert!(handle(&mut state, &document, key(KeyCode::Char('G')), 10));
    assert_eq!(state.scroll, 12);
    assert!(handle(&mut state, &document, key(KeyCode::Char('k')), 10));
    assert_eq!(state.scroll, 11);
    assert!(handle(&mut state, &document, key(KeyCode::Home), 10));
    assert_eq!(state.scroll, 0);
    assert!(handle(&mut state, &document, key(KeyCode::Char('k')), 10));
    assert_eq!(state.scroll, 0);
  }

  #[test]
  fn half_page_keys_move_by_at_least_one_row() {
    let document = document(1, 20);
    let mut state = DiffViewState::default();
    assert!(handle(
      &mut state,
      &document,
      control(KeyCode::Char('d')),
      10
    ));
    assert_eq!(state.scroll, 5);
    assert!(handle(&mut state, &document, key(KeyCode::PageUp), 10));
    assert_eq!(state.scroll, 0);
    assert!(handle(&mut state, &document, key(KeyCode::PageDown), 1));
    assert_eq!(state.scroll, 1);
    assert!(handle(
      &mut state,
      &document,
      control(KeyCode::Char('u')),
      1
    ));
    assert_eq!(state.scroll, 0);
  }

  #[test]
  fn letter_scrolling_and_horizontal_offsets_saturate() {
    let document = document(1, 20);
    let mut state = DiffViewState::default();
    assert!(handle(
      &mut state,
      &document,
      control(KeyCode::Char('n')),
      10
    ));
    assert_eq!(state.scroll, 1);
    assert!(handle(
      &mut state,
      &document,
      control(KeyCode::Char('p')),
      10
    ));
    assert_eq!(state.scroll, 0);
    assert!(handle(&mut state, &document, key(KeyCode::Left), 10));
    assert_eq!(state.h_scroll, 0);
    assert!(handle(&mut state, &document, key(KeyCode::Char('l')), 10));
    assert_eq!(state.h_scroll, 4);
    assert!(handle(&mut state, &document, key(KeyCode::Right), 10));
    assert_eq!(state.h_scroll, 8);
  }

  #[test]
  fn file_keys_move_the_selection_and_reset_the_scroll() {
    let document = document(3, 20);
    let mut state = DiffViewState {
      scroll: 7,
      ..DiffViewState::default()
    };
    assert!(handle(&mut state, &document, key(KeyCode::Char('n')), 10));
    assert_eq!((state.file, state.scroll), (1, 0));
    assert!(handle(&mut state, &document, key(KeyCode::Char(']')), 10));
    assert_eq!(state.file, 2);
    assert!(handle(&mut state, &document, key(KeyCode::Char(']')), 10));
    assert_eq!(state.file, 2);
    assert!(handle(&mut state, &document, key(KeyCode::Char('p')), 10));
    assert_eq!(state.file, 1);
    assert!(handle(&mut state, &document, key(KeyCode::Char('[')), 10));
    assert_eq!(state.file, 0);
    assert!(handle(&mut state, &document, key(KeyCode::Char('[')), 10));
    assert_eq!(state.file, 0);
  }

  #[test]
  fn file_keys_survive_an_empty_document() {
    let document = document(0, 0);
    let mut state = DiffViewState {
      file: 4,
      scroll: 3,
      ..DiffViewState::default()
    };
    assert!(handle(&mut state, &document, key(KeyCode::Char('n')), 10));
    assert_eq!((state.file, state.scroll), (0, 0));
    assert!(handle(&mut state, &document, key(KeyCode::Char('G')), 10));
    assert_eq!(state.scroll, 0);
  }

  #[test]
  fn tab_switches_mode_and_reclamps_the_scroll() {
    let document = document(1, 20);
    let mut state = DiffViewState {
      scroll: 12,
      h_scroll: 8,
      ..DiffViewState::default()
    };
    assert!(handle(&mut state, &document, key(KeyCode::Tab), 10));
    assert_eq!(state.mode, DiffMode::SideBySide);
    assert_eq!(state.h_scroll, 0);
    assert!(state.scroll <= 12);
    assert!(handle(&mut state, &document, key(KeyCode::Tab), 10));
    assert_eq!(state.mode, DiffMode::Unified);
  }

  #[test]
  fn unrelated_keys_are_left_to_the_caller() {
    let document = document(1, 20);
    let mut state = DiffViewState::default();
    for code in [
      KeyCode::Char('x'),
      KeyCode::Esc,
      KeyCode::Char('q'),
      KeyCode::Enter,
    ] {
      assert!(!handle(&mut state, &document, key(code), 10));
    }
    assert_eq!(state, DiffViewState::default());
  }
}
