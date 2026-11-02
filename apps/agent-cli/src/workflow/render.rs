use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::Text,
  widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use super::{ApprovalSelection, TaskSelection, safe_text, task::task_id};
use crate::theme::Theme;

pub fn render_approvals(
  frame: &mut Frame,
  state: &ApprovalSelection,
  area: Rect,
  theme: Theme,
  scroll: u16,
) {
  let items = state
    .rows()
    .iter()
    .map(|row| {
      (
        row.id.clone(),
        format!(
          "{} · {} · {}",
          safe_text(&row.id),
          safe_text(&row.tool),
          safe_text(&row.status)
        ),
      )
    })
    .collect::<Vec<_>>();
  render(
    frame,
    "审批 · ↑↓ 选择 · Space 展开 · Ctrl+Y/E 批准一次/拒绝",
    &items,
    state.selected_id(),
    state.detail(),
    area,
    theme,
    scroll,
  );
}

pub fn render_tasks(frame: &mut Frame, state: &TaskSelection, area: Rect, theme: Theme) {
  let items = state
    .rows()
    .iter()
    .filter_map(|row| {
      let id = task_id(row)?;
      Some((
        id.to_owned(),
        format!(
          "{} · {}",
          safe_text(id),
          safe_text(row["status"].as_str().unwrap_or("unknown"))
        ),
      ))
    })
    .collect::<Vec<_>>();
  render(
    frame,
    "Persistent Tasks",
    &items,
    state.selected_id(),
    state.detail(),
    area,
    theme,
    0,
  );
}

#[allow(clippy::too_many_arguments)]
fn render(
  frame: &mut Frame,
  title: &str,
  items: &[(String, String)],
  selected: Option<&str>,
  detail: Text<'static>,
  area: Rect,
  theme: Theme,
  scroll: u16,
) {
  let [list_area, detail_area] =
    Layout::vertical([Constraint::Length(7), Constraint::Min(0)]).areas(area);
  let mut list_state = ListState::default();
  list_state.select(
    items
      .iter()
      .position(|(id, _)| Some(id.as_str()) == selected),
  );
  let rows = items
    .iter()
    .map(|(_, label)| ListItem::new(label.as_str()))
    .collect::<Vec<_>>();
  frame.render_stateful_widget(
    List::new(rows)
      .block(Block::default().title(title).borders(Borders::BOTTOM))
      .style(Style::default().fg(theme.text))
      .highlight_symbol("› ")
      .highlight_style(
        Style::default()
          .fg(theme.accent)
          .add_modifier(Modifier::BOLD),
      ),
    list_area,
    &mut list_state,
  );
  frame.render_widget(
    Paragraph::new(detail)
      .style(Style::default().fg(theme.text))
      .wrap(Wrap { trim: false })
      .scroll((scroll, 0)),
    detail_area,
  );
}
