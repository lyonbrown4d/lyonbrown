use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::{
  OverlayState, UiIntent, command_palette::matching_commands, status_line_safe, ui_intent,
};
use super::TuiState;
use crate::tui::overlay::help_max_scroll;

impl TuiState {
  pub fn input(&self) -> &str {
    &self.input
  }

  pub fn overlay(&self) -> &OverlayState {
    &self.overlay
  }

  pub fn open_palette(&mut self) {
    if !matches!(self.overlay, OverlayState::Palette { .. }) {
      self.overlay = OverlayState::Palette {
        query: String::new(),
        selected: 0,
      };
    }
  }

  pub fn open_help(&mut self) {
    self.overlay = OverlayState::Help { scroll: 0 };
  }

  pub fn close_overlay(&mut self) {
    self.overlay = OverlayState::Closed;
  }

  pub fn apply_overlay_key(&mut self, key: KeyEvent, viewport_height: u16) -> UiIntent {
    match ui_intent(key, self.input.is_empty(), self.overlay.is_open()) {
      UiIntent::CloseOverlay => {
        self.close_overlay();
        return UiIntent::CloseOverlay;
      }
      UiIntent::OpenPalette => {
        self.open_palette();
        return UiIntent::OpenPalette;
      }
      _ => {}
    }
    match &mut self.overlay {
      OverlayState::Palette { query, selected } => apply_palette_key(query, selected, key),
      OverlayState::Help { scroll } => {
        let max_scroll = help_max_scroll(viewport_height);
        *scroll = match key.code {
          KeyCode::Up => scroll.saturating_sub(1),
          KeyCode::Down => scroll.saturating_add(1).min(max_scroll),
          KeyCode::PageUp => scroll.saturating_sub(5),
          KeyCode::PageDown => scroll.saturating_add(5).min(max_scroll),
          KeyCode::Home => 0,
          KeyCode::End => max_scroll,
          _ => (*scroll).min(max_scroll),
        };
        UiIntent::None
      }
      OverlayState::Closed => UiIntent::None,
    }
  }
}

fn apply_palette_key(query: &mut String, selected: &mut usize, key: KeyEvent) -> UiIntent {
  match key.code {
    KeyCode::Char(character)
      if !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
    {
      query.push_str(&status_line_safe(&character.to_string()));
      *selected = 0;
    }
    KeyCode::Backspace => {
      query.pop();
      *selected = 0;
    }
    KeyCode::Up => *selected = selected.saturating_sub(1),
    KeyCode::Down => {
      *selected = selected
        .saturating_add(1)
        .min(matching_commands(query).len().saturating_sub(1));
    }
    KeyCode::Enter => {
      if let Some(command) = matching_commands(query).get(*selected) {
        return UiIntent::Activate(command.id);
      }
    }
    _ => {}
  }
  UiIntent::None
}
