use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{
  ApprovalRequest, Locale, Speaker, Theme, TuiState,
  composer::{render_approval, render_composer},
  locale::Messages,
  model_setup::{ModelSetupRenderContext, render_model_setup},
  overlay::render_overlay,
};

use chrome::{render_context_line, render_status_line};

mod chrome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderContext {
  pub theme: Theme,
  pub locale: Locale,
  pub setup_command_pending: bool,
  pub setup_exit_pending: bool,
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
  context: RenderContext,
) -> RenderMetrics {
  render_surface(
    frame,
    state,
    approval,
    approval_scroll,
    conversation_scroll,
    context,
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

  let setup_active = state.model_setup().is_some();
  let composer_height = if setup_active {
    0
  } else {
    area.height.saturating_sub(3).min(4)
  };
  let rows = Layout::vertical([
    Constraint::Length(1),
    Constraint::Min(1),
    Constraint::Length(composer_height),
    Constraint::Length(1),
  ])
  .split(area);
  render_context_line(frame, rows[0], state, theme, messages);
  let conversation_max_scroll = if let Some(setup) = state.model_setup() {
    render_model_setup(
      frame,
      rows[1],
      setup,
      ModelSetupRenderContext {
        command_error: state.command_error(),
        commands_enabled: state.can_issue_setup_commands() && !context.setup_command_pending,
        command_pending: context.setup_command_pending,
        exit_pending: context.setup_exit_pending,
        theme,
        messages,
      },
    );
    0
  } else {
    render_conversation(frame, rows[1], state, conversation_scroll, theme, messages)
  };
  if rows[2].height > 0 {
    render_composer(frame, rows[2], state, approval.is_none(), theme, messages);
  }
  render_status_line(frame, rows[3], state, theme, messages);
  let approval_max_scroll = approval
    .map(|request| render_approval(frame, request, approval_scroll, theme, messages))
    .unwrap_or(0);
  if approval.is_none() && state.model_setup().is_none() {
    render_overlay(frame, state, theme, context.locale);
  }
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
