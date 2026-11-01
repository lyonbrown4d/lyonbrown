//! Overlay selector for command results that need a choice.
//!
//! Selection is a transient composer overlay: it never owns daemon authority and never becomes a
//! navigation route. Confirming an item only produces an `Action` for the caller to dispatch.

use agent_protocol::v1::ModelCandidate;
use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::Clear,
};
use ratatui_interact::components::{ListPicker, ListPickerState, ListPickerStyle};

use crate::theme::Theme;

/// What confirming the highlighted row dispatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerValue {
  Model { model: String, execution: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerItem {
  pub label: String,
  pub detail: String,
  pub value: PickerValue,
}

/// `ListPicker::new` requires `Display`; rows render through `render_item` instead.
impl std::fmt::Display for PickerItem {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(formatter, "{}", self.label)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
  pub title: String,
  pub items: Vec<PickerItem>,
  pub selected: usize,
  pub loading: bool,
  pub error: String,
}

impl Picker {
  pub fn models() -> Self {
    Self {
      title: "选择模型 · ↑↓ 选择 · Enter 确认 · Esc 取消".into(),
      items: Vec::new(),
      selected: 0,
      loading: true,
      error: String::new(),
    }
  }

  /// One row per model execution, so a model with several runtimes stays unambiguous.
  pub fn with_models(mut self, models: &[ModelCandidate]) -> Self {
    self.loading = false;
    self.items = models
      .iter()
      .flat_map(|model| {
        let current = model.selected;
        model
          .executions
          .iter()
          .map(move |execution| PickerItem {
            label: format!("{} · {}", model.display_name, execution.label),
            detail: format!(
              "{} · {} · {}",
              model.vendor,
              execution.transport,
              model_state(&model.state, &execution.reason)
            ),
            value: PickerValue::Model {
              model: model.id.clone(),
              execution: execution.id.clone(),
            },
          })
          .map(|item| (current, item))
          .collect::<Vec<_>>()
      })
      .enumerate()
      .map(|(index, (selected, mut item))| {
        if selected && index == 0 {
          item.detail = format!("当前 · {}", item.detail);
        }
        item
      })
      .collect();
    self
  }

  pub fn is_empty(&self) -> bool {
    self.items.is_empty()
  }

  pub fn selected_value(&self) -> Option<&PickerValue> {
    self.items.get(self.selected).map(|item| &item.value)
  }

  pub fn next(&mut self) {
    if !self.items.is_empty() {
      self.selected = (self.selected + 1).min(self.items.len() - 1);
    }
  }

  pub fn previous(&mut self) {
    self.selected = self.selected.saturating_sub(1);
  }

  /// Rows shown by the overlay; the caller sizes the popup with the same number.
  pub fn visible_rows(&self) -> usize {
    self.items.len().clamp(1, 8)
  }
}

fn model_state(state: &str, reason: &str) -> String {
  match state {
    "ready" => "可用".into(),
    "unavailable" => "不可用".into(),
    _ if reason.is_empty() => "需要凭据".into(),
    _ => format!("需要凭据 · {reason}"),
  }
}

pub fn render(frame: &mut Frame, area: Rect, picker: &Picker, theme: Theme) {
  frame.render_widget(Clear, area);
  let style = ListPickerStyle {
    selected_style: Style::default()
      .fg(theme.accent)
      .add_modifier(Modifier::BOLD),
    normal_style: Style::default().fg(theme.text),
    indicator_style: Style::default().fg(theme.accent),
    border_style: Style::default().fg(theme.accent),
    indicator: "› ",
    indicator_empty: "  ",
    bordered: true,
  };
  let mut state = ListPickerState::new(picker.items.len());
  state.select(picker.selected);
  state.ensure_visible(area.height.saturating_sub(3) as usize);
  let items = picker.items.clone();
  frame.render_widget(
    ListPicker::new(&items, &state)
      .render_item(|item, _index, _selected| {
        vec![Line::from(vec![
          Span::raw(format!("{:<28}", item.label)),
          Span::styled(item.detail.clone(), Style::default().fg(theme.muted)),
        ])]
      })
      .title(&picker.title)
      .style(style),
    area,
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use agent_protocol::v1::ModelExecution;

  fn candidate(selected: bool) -> ModelCandidate {
    ModelCandidate {
      id: "deepseek-chat".into(),
      display_name: "DeepSeek Chat".into(),
      vendor: "deepseek".into(),
      source: "builtin".into(),
      state: "configurable".into(),
      selected,
      executions: vec![ModelExecution {
        id: "builtin:deepseek-chat".into(),
        label: "API".into(),
        transport: "api".into(),
        state: "configurable".into(),
        reason: String::new(),
      }],
    }
  }

  #[test]
  fn builds_one_row_per_execution_and_marks_the_current_model() {
    let picker = Picker::models().with_models(&[candidate(true), candidate(false)]);
    assert!(!picker.loading);
    assert_eq!(picker.items.len(), 2);
    assert!(picker.items[0].detail.starts_with("当前"));
    assert_eq!(
      picker.items[0].value,
      PickerValue::Model {
        model: "deepseek-chat".into(),
        execution: "builtin:deepseek-chat".into(),
      }
    );
  }

  #[test]
  fn selection_stays_inside_the_item_range() {
    let mut picker = Picker::models().with_models(&[candidate(false)]);
    picker.next();
    picker.next();
    assert_eq!(picker.selected, 0);
    picker.previous();
    assert_eq!(picker.selected, 0);
  }
}
