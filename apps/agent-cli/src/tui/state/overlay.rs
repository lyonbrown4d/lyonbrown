use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::{
  ActivityFocus, ActivityInspectorState, OverlayState, UiIntent,
  command_palette::matching_commands, status_line_safe, ui_intent,
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

  pub fn open_activity(&mut self) {
    self.overlay = OverlayState::Activity(ActivityInspectorState::latest(&self.activities));
  }

  pub fn close_overlay(&mut self) {
    self.overlay = OverlayState::Closed;
  }

  pub fn apply_overlay_key(&mut self, key: KeyEvent, viewport_height: u16) -> UiIntent {
    self.apply_overlay_key_with_viewport(key, 90, viewport_height)
  }

  pub fn apply_overlay_key_with_viewport(
    &mut self,
    key: KeyEvent,
    viewport_width: u16,
    viewport_height: u16,
  ) -> UiIntent {
    match ui_intent(key, self.input.is_empty(), self.overlay.is_open()) {
      UiIntent::CloseOverlay => {
        self.close_overlay();
        return UiIntent::CloseOverlay;
      }
      UiIntent::OpenPalette => {
        self.open_palette();
        return UiIntent::OpenPalette;
      }
      UiIntent::ToggleActivity => {
        if matches!(self.overlay, OverlayState::Activity(_)) {
          self.close_overlay();
        } else {
          self.open_activity();
        }
        return UiIntent::ToggleActivity;
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
      OverlayState::Activity(inspector) => {
        apply_activity_key(
          inspector,
          &self.activities,
          key,
          viewport_width,
          viewport_height,
        );
        UiIntent::None
      }
      OverlayState::Closed => UiIntent::None,
    }
  }

  pub(super) fn reconcile_activity_selection(&mut self) {
    let OverlayState::Activity(inspector) = &mut self.overlay else {
      return;
    };
    let latest = self.activities.last().map(|item| item.target);
    if inspector.follow_latest {
      if inspector.selected != latest {
        inspector.detail_scroll = 0;
      }
      inspector.selected = latest;
    } else if !self
      .activities
      .iter()
      .any(|item| Some(item.target) == inspector.selected)
    {
      inspector.selected = latest;
      inspector.detail_scroll = 0;
      inspector.follow_latest = true;
    }
  }

  pub(super) fn reset_activity_inspector(&mut self) {
    if matches!(self.overlay, OverlayState::Activity(_)) {
      self.close_overlay();
    }
    self.activities.clear();
  }

  #[doc(hidden)]
  pub fn clamp_activity_detail_scroll(&mut self, max_scroll: u16) {
    if let OverlayState::Activity(inspector) = &mut self.overlay {
      inspector.detail_scroll = inspector.detail_scroll.min(max_scroll);
    }
  }
}

fn apply_activity_key(
  inspector: &mut ActivityInspectorState,
  activities: &[super::super::ActivityItem],
  key: KeyEvent,
  viewport_width: u16,
  viewport_height: u16,
) {
  let page = activity_page_size(viewport_width, viewport_height, inspector.focus);
  match inspector.focus {
    ActivityFocus::List => match key.code {
      KeyCode::Up => move_selection(inspector, activities, 1, false),
      KeyCode::Down => move_selection(inspector, activities, 1, true),
      KeyCode::PageUp => move_selection(inspector, activities, page, false),
      KeyCode::PageDown => move_selection(inspector, activities, page, true),
      KeyCode::Enter | KeyCode::Right | KeyCode::Tab => {
        inspector.focus = ActivityFocus::Detail;
        inspector.detail_scroll = 0;
      }
      _ => {}
    },
    ActivityFocus::Detail => match key.code {
      KeyCode::Up => inspector.detail_scroll = inspector.detail_scroll.saturating_sub(1),
      KeyCode::Down => inspector.detail_scroll = inspector.detail_scroll.saturating_add(1),
      KeyCode::PageUp => {
        inspector.detail_scroll = inspector.detail_scroll.saturating_sub(page as u16)
      }
      KeyCode::PageDown => {
        inspector.detail_scroll = inspector.detail_scroll.saturating_add(page as u16)
      }
      KeyCode::Home => inspector.detail_scroll = 0,
      KeyCode::End => inspector.detail_scroll = u16::MAX,
      KeyCode::Left | KeyCode::BackTab => inspector.focus = ActivityFocus::List,
      _ => {}
    },
  }
}

fn activity_page_size(width: u16, height: u16, focus: ActivityFocus) -> usize {
  let main_height = height.saturating_sub(3);
  let pane_height = if width >= 90 && height >= 20 || width < 52 {
    main_height
  } else if focus == ActivityFocus::List {
    main_height.saturating_mul(38) / 100
  } else {
    main_height.saturating_mul(62) / 100
  };
  usize::from(pane_height.saturating_sub(2).max(1))
}

fn move_selection(
  inspector: &mut ActivityInspectorState,
  activities: &[super::super::ActivityItem],
  amount: usize,
  forward: bool,
) {
  if activities.is_empty() {
    inspector.selected = None;
    return;
  }
  let current = inspector
    .selected
    .and_then(|target| activities.iter().position(|item| item.target == target))
    .unwrap_or(activities.len() - 1);
  let next = if forward {
    current.saturating_add(amount).min(activities.len() - 1)
  } else {
    current.saturating_sub(amount)
  };
  if inspector.selected != Some(activities[next].target) {
    inspector.detail_scroll = 0;
  }
  inspector.selected = Some(activities[next].target);
  inspector.follow_latest = next == activities.len() - 1;
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
