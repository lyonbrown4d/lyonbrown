use agent_core::SandboxStatus;
use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::Style,
  text::Line,
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::{ApprovalRequest, TuiState, approval_safe, locale::Messages, theme::Theme, ui::panel};

pub(super) fn render_composer(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  interactive: bool,
  theme: Theme,
  messages: Messages,
) {
  let (mode, hint) = if state.is_model_setup_required() {
    (messages.model_setup, messages.model_setup_hint)
  } else if !state.can_submit() {
    (messages.unavailable, messages.connection_wait_hint)
  } else if state.busy {
    (messages.running, messages.running_hint)
  } else {
    (messages.ready, messages.ready_hint)
  };
  let hint = state.queued_follow_up().map_or_else(
    || hint.to_owned(),
    |queued| format!("{hint} · {}: {queued}", messages.queued_next),
  );
  let title = format!(" {mode} · {} ", messages.message);
  let block = panel(&title, theme, true);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if inner.is_empty() {
    return;
  }

  if state.input.is_empty() {
    frame.render_widget(
      Paragraph::new(hint.as_str()).style(Style::default().fg(theme.text_secondary)),
      inner,
    );
    if interactive {
      frame.set_cursor_position((inner.x, inner.y));
    }
    return;
  }

  let sections = if inner.height > 1 {
    Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner)
  } else {
    Layout::vertical([Constraint::Min(1), Constraint::Length(0)]).split(inner)
  };
  let input_area = sections[0];
  let prefix = &state.input[..state.input_cursor.min(state.input.len())];
  let cursor_row = prefix
    .chars()
    .filter(|character| *character == '\n')
    .count();
  let current_line = prefix.rsplit('\n').next().unwrap_or_default();
  let cursor_column = Line::from(current_line).width();
  let vertical_scroll = cursor_row.saturating_sub(usize::from(input_area.height.saturating_sub(1)));
  let horizontal_scroll =
    cursor_column.saturating_sub(usize::from(input_area.width.saturating_sub(1)));
  frame.render_widget(
    Paragraph::new(state.input.as_str())
      .style(Style::default().fg(theme.text_primary))
      .scroll((as_u16(vertical_scroll), as_u16(horizontal_scroll))),
    input_area,
  );
  if sections[1].height > 0 {
    frame.render_widget(
      Paragraph::new(hint).style(Style::default().fg(theme.text_faint)),
      sections[1],
    );
  }
  if interactive && !input_area.is_empty() {
    frame.set_cursor_position((
      input_area.x + as_u16(cursor_column.saturating_sub(horizontal_scroll)),
      input_area.y + as_u16(cursor_row.saturating_sub(vertical_scroll)),
    ));
  }
}

fn as_u16(value: usize) -> u16 {
  value.min(usize::from(u16::MAX)) as u16
}

pub(super) fn render_approval(
  frame: &mut Frame<'_>,
  request: &ApprovalRequest,
  requested_scroll: u16,
  theme: Theme,
  messages: Messages,
) -> u16 {
  let area = if frame.area().width < 70 || frame.area().height < 18 {
    frame.area()
  } else {
    centered_rect(76, 70, frame.area())
  };
  frame.render_widget(Clear, area);
  let preview = &request.preview;
  let sandbox = match preview.sandbox {
    SandboxStatus::WorkspaceRestricted => "workspace-restricted",
    SandboxStatus::HostUnsandboxed => "host/unsandboxed",
    SandboxStatus::Unknown => "unknown",
  };
  let text = format!(
    "{}: {}\n{}: {:?}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}",
    messages.tool,
    approval_safe(&preview.tool),
    messages.risk,
    preview.risk,
    messages.action,
    approval_safe(&preview.action),
    messages.target,
    approval_safe(&preview.target),
    messages.impact,
    approval_safe(&preview.impact),
    messages.working_directory,
    preview.working_directory.as_ref().map_or_else(
      || messages.none.into(),
      |path| approval_safe(&path.display().to_string())
    ),
    messages.sandbox,
    sandbox,
    messages.secret_references,
    if preview.required_secrets.is_empty() {
      messages.none.to_owned()
    } else {
      preview
        .required_secrets
        .iter()
        .map(ToString::to_string)
        .map(|name| approval_safe(&name))
        .collect::<Vec<_>>()
        .join(", ")
    }
  );
  let block = Block::default()
    .title(format!(" {} ", messages.approval_required))
    .borders(Borders::ALL)
    .border_style(Style::default().fg(theme.status_warning))
    .style(
      Style::default()
        .bg(theme.surface_raised)
        .fg(theme.text_primary),
    );
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if inner.is_empty() {
    return 0;
  }
  let footer_height = if inner.height > 2 { 2 } else { 0 };
  let sections =
    Layout::vertical([Constraint::Min(1), Constraint::Length(footer_height)]).split(inner);
  let body = sections[0];
  let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
  let line_count = paragraph.line_count(body.width.max(1));
  let max_scroll = line_count
    .saturating_sub(usize::from(body.height))
    .min(usize::from(u16::MAX)) as u16;
  let scroll = requested_scroll.min(max_scroll);
  frame.render_widget(paragraph.scroll((scroll, 0)), body);
  if footer_height > 0 {
    let first = usize::from(scroll).saturating_add(1).min(line_count.max(1));
    let last = usize::from(scroll)
      .saturating_add(usize::from(body.height))
      .min(line_count);
    frame.render_widget(
      Paragraph::new(vec![
        Line::from(messages.approval_hint),
        Line::from(format!(
          "lines {first}-{last}/{} · ↑↓ PgUp/PgDn",
          line_count.max(1)
        )),
      ])
      .style(Style::default().fg(theme.status_warning)),
      sections[1],
    );
  }
  max_scroll
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
  let width = area.width.saturating_mul(width_percent) / 100;
  let height = area.height.saturating_mul(height_percent) / 100;
  Rect::new(
    area.x + area.width.saturating_sub(width) / 2,
    area.y + area.height.saturating_sub(height) / 2,
    width,
    height,
  )
}
