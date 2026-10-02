use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Text},
  widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
  },
};

use super::{
  ActivityFocus, ActivityInspectorState, Locale, Theme, TuiState, activity_detail::phase_label,
  locale::ActivityMessages,
};

pub(super) fn render_activity(
  frame: &mut Frame<'_>,
  state: &TuiState,
  inspector: &ActivityInspectorState,
  theme: Theme,
  locale: Locale,
) -> u16 {
  let area = frame.area();
  let messages = locale.messages().inspector;
  frame.render_widget(Clear, area);
  let freshness = if state.is_snapshot_stale() {
    messages.stale
  } else {
    messages.fresh
  };
  let title = format!(" {freshness} · {} ", messages.title);
  let outer = Block::default()
    .title(title)
    .borders(Borders::ALL)
    .border_style(Style::default().fg(theme.focus_ring))
    .style(Style::default().bg(theme.background).fg(theme.text_primary));
  let inner = outer.inner(area);
  frame.render_widget(outer, area);
  if inner.is_empty() {
    return 0;
  }
  let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
  let detail_max_scroll = match layout_mode(area) {
    InspectorLayout::Dual => {
      let columns =
        Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)]).split(rows[0]);
      render_list(frame, columns[0], state, inspector, theme, messages);
      render_detail(frame, columns[1], state, inspector, theme, locale, messages)
    }
    InspectorLayout::Stacked => {
      let panes =
        Layout::vertical([Constraint::Percentage(38), Constraint::Percentage(62)]).split(rows[0]);
      render_list(frame, panes[0], state, inspector, theme, messages);
      render_detail(frame, panes[1], state, inspector, theme, locale, messages)
    }
    InspectorLayout::Single => match inspector.focus {
      ActivityFocus::List => {
        render_list(frame, rows[0], state, inspector, theme, messages);
        0
      }
      ActivityFocus::Detail => {
        render_detail(frame, rows[0], state, inspector, theme, locale, messages)
      }
    },
  };
  let hint = match inspector.focus {
    ActivityFocus::List => messages.list_hint,
    ActivityFocus::Detail => messages.detail_hint,
  };
  frame.render_widget(
    Paragraph::new(hint).style(Style::default().fg(theme.text_faint)),
    rows[1],
  );
  detail_max_scroll
}

#[derive(Clone, Copy)]
enum InspectorLayout {
  Dual,
  Stacked,
  Single,
}

fn layout_mode(area: Rect) -> InspectorLayout {
  if area.width >= 90 && area.height >= 20 {
    InspectorLayout::Dual
  } else if area.width >= 52 {
    InspectorLayout::Stacked
  } else {
    InspectorLayout::Single
  }
}

fn render_list(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  inspector: &ActivityInspectorState,
  theme: Theme,
  messages: ActivityMessages,
) {
  let selected = inspector.selected.and_then(|target| {
    state
      .activities()
      .iter()
      .position(|item| item.target == target)
  });
  let items = state
    .activities()
    .iter()
    .enumerate()
    .map(|(index, item)| {
      ListItem::new(format!(
        "{}  {}",
        index + 1,
        phase_label(item.phase, messages)
      ))
    })
    .collect::<Vec<_>>();
  let focused = inspector.focus == ActivityFocus::List;
  let list = List::new(items)
    .block(pane(messages.steps, theme, focused))
    .highlight_symbol("› ")
    .highlight_style(
      Style::default()
        .fg(theme.accent_primary)
        .add_modifier(Modifier::BOLD),
    );
  let mut list_state = ListState::default().with_selected(selected);
  frame.render_stateful_widget(list, area, &mut list_state);
  if state.activities().is_empty() {
    frame.render_widget(
      Paragraph::new(messages.empty)
        .style(Style::default().fg(theme.text_faint))
        .block(pane(messages.steps, theme, focused)),
      area,
    );
  }
}

fn render_detail(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  inspector: &ActivityInspectorState,
  theme: Theme,
  locale: Locale,
  messages: ActivityMessages,
) -> u16 {
  let block = pane(
    messages.detail,
    theme,
    inspector.focus == ActivityFocus::Detail,
  );
  let inner = block.inner(area);
  let detail = state.activity_detail(locale);
  let lines = if detail.is_empty() {
    vec![Line::from(messages.empty)]
  } else {
    detail
      .iter()
      .flat_map(|line| line.split('\n').map(str::to_owned).collect::<Vec<_>>())
      .map(Line::from)
      .collect::<Vec<_>>()
  };
  let text = Text::from(lines);
  let rendered_lines = Paragraph::new(text.clone())
    .wrap(Wrap { trim: false })
    .line_count(inner.width.max(1));
  let max_scroll = rendered_lines
    .saturating_sub(usize::from(inner.height))
    .min(usize::from(u16::MAX)) as u16;
  let scroll = inspector.detail_scroll.min(max_scroll);
  let paragraph = Paragraph::new(text)
    .block(block)
    .wrap(Wrap { trim: false })
    .scroll((scroll, 0));
  frame.render_widget(paragraph, area);
  if max_scroll > 0 {
    let mut scrollbar = ScrollbarState::new(rendered_lines)
      .viewport_content_length(usize::from(inner.height))
      .position(usize::from(scroll));
    frame.render_stateful_widget(
      Scrollbar::new(ScrollbarOrientation::VerticalRight),
      area,
      &mut scrollbar,
    );
  }
  max_scroll
}

fn pane(title: &'static str, theme: Theme, focused: bool) -> Block<'static> {
  Block::default()
    .title(title)
    .borders(Borders::ALL)
    .border_style(Style::default().fg(if focused {
      theme.focus_ring
    } else {
      theme.border_default
    }))
    .style(Style::default().bg(theme.background).fg(theme.text_primary))
}
