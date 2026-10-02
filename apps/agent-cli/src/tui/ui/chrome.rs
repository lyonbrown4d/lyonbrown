use ratatui::{
  Frame,
  layout::Rect,
  style::Style,
  text::{Line, Span},
  widgets::Paragraph,
};

use crate::tui::{Theme, TuiState, connection::connection_label, locale::Messages};

pub(super) fn render_context_line(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  theme: Theme,
  messages: Messages,
) {
  let context = format!(
    " {} · {} · {} · {} · {} · {}{} ",
    messages.workspace,
    messages.auto,
    messages.local,
    state.model(),
    connection_label(state.connection(), messages),
    if state.is_snapshot_stale() {
      messages.stale
    } else {
      messages.fresh
    },
    state
      .connection_error()
      .map(|error| format!(" · {error}"))
      .unwrap_or_default()
  );
  frame.render_widget(
    Paragraph::new(context).style(
      Style::default()
        .fg(theme.text_secondary)
        .bg(theme.surface_raised),
    ),
    area,
  );
}

pub(super) fn render_status_line(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  theme: Theme,
  messages: Messages,
) {
  let (symbol, mode, color, actions) = if state.is_model_setup_required() {
    (
      "○",
      messages.model_setup,
      theme.status_warning,
      messages.model_setup_hint.to_owned(),
    )
  } else if !state.can_submit() {
    (
      "○",
      connection_label(state.connection(), messages),
      theme.status_warning,
      messages.palette.offline_status_hint.to_owned(),
    )
  } else if state.busy {
    (
      "◉",
      messages.running,
      theme.status_warning,
      format!(
        "Ctrl-C {} · {}",
        messages.cancel, messages.palette.ready_status_hint
      ),
    )
  } else {
    (
      "●",
      messages.ready,
      theme.status_success,
      messages.palette.ready_status_hint.to_owned(),
    )
  };
  frame.render_widget(
    Paragraph::new(Line::from(vec![
      Span::styled(format!(" {symbol} {mode} "), Style::default().fg(color)),
      Span::styled(
        format!("· {actions}"),
        Style::default().fg(theme.text_faint),
      ),
    ]))
    .style(Style::default().bg(theme.surface_raised)),
    area,
  );
}
