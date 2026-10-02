use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{
  ApprovalRequest, Locale, Speaker, Theme, TuiConnectionState, TuiState,
  composer::{render_approval, render_composer},
  locale::Messages,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderContext {
  pub theme: Theme,
  pub locale: Locale,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RenderMetrics {
  pub approval_max_scroll: u16,
  pub conversation_max_scroll: u16,
}

pub fn render(frame: &mut Frame<'_>, state: &TuiState) {
  render_with_context(frame, state, RenderContext::default());
}

pub fn render_with_context(frame: &mut Frame<'_>, state: &TuiState, context: RenderContext) {
  let _ = render_surface(frame, state, None, 0, 0, context);
}

pub fn render_with_scroll(frame: &mut Frame<'_>, state: &TuiState, conversation_scroll: u16) {
  let _ = render_surface(
    frame,
    state,
    None,
    0,
    conversation_scroll,
    RenderContext::default(),
  );
}

#[doc(hidden)]
pub fn render_with_approval(
  frame: &mut Frame<'_>,
  state: &TuiState,
  approval: Option<&ApprovalRequest>,
  approval_scroll: u16,
) -> u16 {
  render_surface(
    frame,
    state,
    approval,
    approval_scroll,
    0,
    RenderContext::default(),
  )
  .approval_max_scroll
}

pub(crate) fn render_cockpit(
  frame: &mut Frame<'_>,
  state: &TuiState,
  approval: Option<&ApprovalRequest>,
  approval_scroll: u16,
  conversation_scroll: u16,
) -> RenderMetrics {
  render_surface(
    frame,
    state,
    approval,
    approval_scroll,
    conversation_scroll,
    RenderContext::default(),
  )
}

fn render_surface(
  frame: &mut Frame<'_>,
  state: &TuiState,
  approval: Option<&ApprovalRequest>,
  approval_scroll: u16,
  conversation_scroll: u16,
  context: RenderContext,
) -> RenderMetrics {
  let area = frame.area();
  let theme = context.theme;
  let messages = context.locale.messages();
  frame.render_widget(
    Block::default().style(Style::default().bg(theme.background)),
    area,
  );
  if area.width < 30 || area.height < 8 {
    frame.render_widget(
      Paragraph::new(messages.terminal_too_small).style(
        Style::default()
          .fg(theme.status_warning)
          .bg(theme.background),
      ),
      area,
    );
    return RenderMetrics::default();
  }

  let composer_height = area.height.saturating_sub(3).min(4);
  let rows = Layout::vertical([
    Constraint::Length(1),
    Constraint::Min(1),
    Constraint::Length(composer_height),
    Constraint::Length(1),
  ])
  .split(area);
  render_context_line(frame, rows[0], state, theme, messages);
  let conversation_max_scroll =
    render_conversation(frame, rows[1], state, conversation_scroll, theme, messages);
  render_composer(
    frame,
    rows[2],
    state,
    approval.is_none() && state.can_submit(),
    theme,
    messages,
  );
  render_status_line(frame, rows[3], state, theme, messages);
  let approval_max_scroll = approval
    .map(|request| render_approval(frame, request, approval_scroll, theme, messages))
    .unwrap_or(0);
  RenderMetrics {
    approval_max_scroll,
    conversation_max_scroll,
  }
}

pub(super) fn panel(title: &str, theme: Theme, focused: bool) -> Block<'_> {
  let border = if focused {
    theme.focus_ring
  } else {
    theme.border_default
  };
  Block::default()
    .title(Span::styled(
      title.to_owned(),
      Style::default()
        .fg(theme.accent_primary)
        .add_modifier(Modifier::BOLD),
    ))
    .borders(Borders::ALL)
    .border_style(Style::default().fg(border))
    .style(Style::default().bg(theme.background).fg(theme.text_primary))
}

fn render_context_line(
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

fn render_conversation(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  distance_from_bottom: u16,
  theme: Theme,
  messages: Messages,
) -> u16 {
  let mut lines = Vec::new();
  for entry in state.transcript() {
    let (label, color) = match entry.speaker {
      Speaker::User => (messages.user, theme.accent_primary),
      Speaker::Assistant => (messages.assistant, theme.accent_secondary),
      Speaker::System => (messages.system, theme.status_warning),
    };
    lines.push(Line::from(Span::styled(
      label,
      Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
    lines.extend(entry.text.split('\n').map(Line::from));
    lines.push(Line::default());
  }
  let content_width = area.width.saturating_sub(2).max(1);
  let text = Text::from(lines);
  let rendered_lines = Paragraph::new(text.clone())
    .wrap(Wrap { trim: false })
    .line_count(content_width);
  let max_scroll = rendered_lines
    .saturating_sub(usize::from(area.height.saturating_sub(2)))
    .min(usize::from(u16::MAX)) as u16;
  let distance_from_bottom = distance_from_bottom.min(max_scroll);
  let position = if distance_from_bottom == 0 {
    messages.following.to_owned()
  } else {
    format!("{distance_from_bottom} {}", messages.from_bottom)
  };
  let title = format!(" {} · {position} · {} ", messages.activity, messages.scroll);
  let paragraph = Paragraph::new(text)
    .block(panel(&title, theme, false))
    .wrap(Wrap { trim: false });
  let scroll = max_scroll.saturating_sub(distance_from_bottom);
  frame.render_widget(paragraph.scroll((scroll, 0)), area);
  max_scroll
}

fn render_status_line(
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
      messages.connection_wait_hint.to_owned(),
    )
  } else if state.busy {
    (
      "◉",
      messages.running,
      theme.status_warning,
      format!("Ctrl-C {} · Ctrl-O {}", messages.cancel, messages.details),
    )
  } else {
    (
      "●",
      messages.ready,
      theme.status_success,
      format!("Ctrl-O {} · /help {}", messages.details, messages.commands),
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

fn connection_label(state: TuiConnectionState, messages: Messages) -> &'static str {
  match state {
    TuiConnectionState::Connecting => messages.connecting,
    TuiConnectionState::Connected => messages.connected,
    TuiConnectionState::Reconnecting => messages.reconnecting,
    TuiConnectionState::Unavailable => messages.unavailable,
    TuiConnectionState::Incompatible => messages.incompatible,
  }
}
