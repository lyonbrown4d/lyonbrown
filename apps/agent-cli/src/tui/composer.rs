use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Paragraph};

use super::{
  TuiState,
  ui::{MUTED_STEEL, PAPER, panel},
};

pub(super) fn render_composer(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  interactive: bool,
) {
  let title = if state.busy {
    " BUSY · input locked "
  } else {
    " IDLE · Message "
  };
  let block = panel(title);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if inner.is_empty() {
    return;
  }

  if state.input.is_empty() {
    frame.render_widget(
      Paragraph::new("Enter send · ←→ move · ↑↓ scroll · Esc quit")
        .style(Style::default().fg(MUTED_STEEL)),
      inner,
    );
    if interactive {
      frame.set_cursor_position((inner.x, inner.y));
    }
    return;
  }

  let prefix = &state.input[..state.input_cursor.min(state.input.len())];
  let cursor_row = prefix
    .chars()
    .filter(|character| *character == '\n')
    .count();
  let current_line = prefix.rsplit('\n').next().unwrap_or_default();
  let cursor_column = Line::from(current_line).width();
  let vertical_scroll = cursor_row.saturating_sub(usize::from(inner.height.saturating_sub(1)));
  let horizontal_scroll = cursor_column.saturating_sub(usize::from(inner.width.saturating_sub(1)));
  frame.render_widget(
    Paragraph::new(state.input.as_str())
      .style(Style::default().fg(PAPER))
      .scroll((as_u16(vertical_scroll), as_u16(horizontal_scroll))),
    inner,
  );
  if interactive {
    frame.set_cursor_position((
      inner.x + as_u16(cursor_column.saturating_sub(horizontal_scroll)),
      inner.y + as_u16(cursor_row.saturating_sub(vertical_scroll)),
    ));
  }
}

fn as_u16(value: usize) -> u16 {
  value.min(usize::from(u16::MAX)) as u16
}
