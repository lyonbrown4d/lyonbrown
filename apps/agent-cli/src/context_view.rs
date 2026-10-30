use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::Text,
  widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{theme::Theme, view_model::TaskListView};

pub fn render_text(
  frame: &mut Frame,
  title: &str,
  content: &Text<'_>,
  scroll: u16,
  area: Rect,
  theme: Theme,
) {
  frame.render_widget(
    Paragraph::new(content.clone())
      .wrap(Wrap { trim: false })
      .scroll((scroll, 0))
      .block(
        Block::default()
          .title(title)
          .borders(Borders::LEFT)
          .border_style(Style::default().fg(theme.muted)),
      ),
    area,
  );
}

pub fn render_task_list(
  frame: &mut Frame,
  title: &str,
  task_list: &TaskListView,
  area: Rect,
  theme: Theme,
) {
  let block = Block::default()
    .title(title)
    .borders(Borders::LEFT)
    .border_style(Style::default().fg(theme.muted));
  let inner = block.inner(area);
  frame.render_widget(block, area);
  let items = task_list
    .items
    .iter()
    .map(|item| ListItem::new(item.as_str()))
    .collect::<Vec<_>>();
  let mut state = ListState::default();
  state.select(Some(task_list.selected.min(items.len().saturating_sub(1))));
  frame.render_stateful_widget(
    List::new(items).highlight_symbol("› ").highlight_style(
      Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD),
    ),
    inner,
    &mut state,
  );
}
