use ratatui::{
  Frame,
  layout::{Constraint, Direction, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::{ApprovalRequest, Speaker, TuiState, composer::render_composer};

const MIDNIGHT_SLATE: Color = Color::Rgb(0x10, 0x18, 0x20);
pub(super) const DEEP_NAVY: Color = Color::Rgb(0x17, 0x2A, 0x3A);
const SIGNAL_CYAN: Color = Color::Rgb(0x36, 0xC5, 0xD0);
const APPROVAL_AMBER: Color = Color::Rgb(0xF2, 0xB8, 0x4B);
pub(super) const PAPER: Color = Color::Rgb(0xE8, 0xEE, 0xF2);
pub(super) const MUTED_STEEL: Color = Color::Rgb(0x78, 0x90, 0x9C);

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RenderMetrics {
  pub approval_max_scroll: u16,
  pub conversation_max_scroll: u16,
}

pub fn render(frame: &mut Frame<'_>, state: &TuiState) {
  let _ = render_cockpit(frame, state, None, 0, 0);
}

pub fn render_with_scroll(frame: &mut Frame<'_>, state: &TuiState, conversation_scroll: u16) {
  let _ = render_cockpit(frame, state, None, 0, conversation_scroll);
}

#[doc(hidden)]
pub fn render_with_approval(
  frame: &mut Frame<'_>,
  state: &TuiState,
  approval: Option<&ApprovalRequest>,
  approval_scroll: u16,
) -> u16 {
  render_cockpit(frame, state, approval, approval_scroll, 0).approval_max_scroll
}

pub(crate) fn render_cockpit(
  frame: &mut Frame<'_>,
  state: &TuiState,
  approval: Option<&ApprovalRequest>,
  approval_scroll: u16,
  conversation_scroll: u16,
) -> RenderMetrics {
  frame.render_widget(
    Block::default().style(Style::default().bg(MIDNIGHT_SLATE)),
    frame.area(),
  );
  let height = frame.area().height;
  let composer_height = height.saturating_sub(1).min(4);
  let rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(1), Constraint::Length(composer_height)])
    .split(frame.area());

  let conversation_max_scroll = if frame.area().width >= 90 {
    let columns = Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Min(35), Constraint::Length(30)])
      .split(rows[0]);
    let max_scroll = render_conversation(frame, columns[0], state, conversation_scroll);
    render_runtime(frame, columns[1], state);
    max_scroll
  } else {
    render_conversation(frame, rows[0], state, conversation_scroll)
  };
  render_composer(frame, rows[1], state, approval.is_none() && !state.busy);
  let approval_max_scroll = if let Some(request) = approval {
    render_approval(frame, request, approval_scroll)
  } else {
    0
  };
  RenderMetrics {
    approval_max_scroll,
    conversation_max_scroll,
  }
}

pub(super) fn panel(title: &str) -> Block<'_> {
  Block::default()
    .title(Span::styled(
      title,
      Style::default()
        .fg(SIGNAL_CYAN)
        .add_modifier(Modifier::BOLD),
    ))
    .borders(Borders::ALL)
    .border_style(Style::default().fg(DEEP_NAVY))
    .style(Style::default().bg(MIDNIGHT_SLATE).fg(PAPER))
}

fn render_conversation(
  frame: &mut Frame<'_>,
  area: Rect,
  state: &TuiState,
  distance_from_bottom: u16,
) -> u16 {
  let mut lines = Vec::new();
  for entry in state.transcript() {
    let (label, color) = match entry.speaker {
      Speaker::User => ("YOU", APPROVAL_AMBER),
      Speaker::Assistant => ("AGENT", SIGNAL_CYAN),
      Speaker::System => ("SYSTEM", APPROVAL_AMBER),
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
    "following".to_owned()
  } else {
    format!("{distance_from_bottom} from bottom")
  };
  let title = format!(" CONVERSATION · {position} · ↑↓ scroll ");
  let paragraph = Paragraph::new(text)
    .block(panel(&title))
    .wrap(Wrap { trim: false });
  let scroll = max_scroll.saturating_sub(distance_from_bottom);
  frame.render_widget(paragraph.scroll((scroll, 0)), area);
  max_scroll
}

fn render_runtime(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
  let (input, output, cache) = state.usage();
  let mut lines = vec![
    Line::from(vec![
      Span::styled("STATUS  ", Style::default().fg(MUTED_STEEL)),
      Span::raw(state.status()),
    ]),
    Line::from(vec![
      Span::styled("MODEL   ", Style::default().fg(MUTED_STEEL)),
      Span::raw(state.model()),
    ]),
    Line::from(vec![
      Span::styled("ROUTE   ", Style::default().fg(MUTED_STEEL)),
      Span::raw(state.reason()),
    ]),
    Line::from(vec![
      Span::styled("STEP    ", Style::default().fg(MUTED_STEEL)),
      Span::raw(state.step()),
    ]),
    Line::from(vec![
      Span::styled("TOOL    ", Style::default().fg(MUTED_STEEL)),
      Span::raw(state.tool()),
    ]),
    Line::from(format!("TOKENS  {input} in / {output} out")),
    Line::from(format!("CACHE   {cache}")),
    Line::default(),
    Line::from(Span::styled(
      "EVENT PULSE",
      Style::default()
        .fg(SIGNAL_CYAN)
        .add_modifier(Modifier::BOLD),
    )),
  ];
  for event in state.pulse().iter().rev() {
    lines.push(Line::from(vec![
      Span::styled("│ ", Style::default().fg(SIGNAL_CYAN)),
      Span::raw(event),
    ]));
  }
  frame.render_widget(
    Paragraph::new(lines)
      .block(panel(" RUNTIME "))
      .wrap(Wrap { trim: true }),
    area,
  );
}

fn render_approval(frame: &mut Frame<'_>, request: &ApprovalRequest, requested_scroll: u16) -> u16 {
  let area = if frame.area().width < 70 || frame.area().height < 18 {
    frame.area()
  } else {
    centered_rect(76, 70, frame.area())
  };
  frame.render_widget(Clear, area);
  let text = format!(
    "tool: {}\nrisk: {:?}\narguments:\n{}\nsecret references: {}",
    request.tool,
    request.risk,
    request.arguments,
    if request.secret_references.is_empty() {
      "none".into()
    } else {
      request.secret_references.join(", ")
    }
  );
  let block = Block::default()
    .title(" APPROVAL REQUIRED ")
    .borders(Borders::ALL)
    .border_style(Style::default().fg(APPROVAL_AMBER))
    .style(Style::default().bg(DEEP_NAVY).fg(PAPER));
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if inner.is_empty() {
    return 0;
  }
  let footer_height = u16::from(inner.height > 1);
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
      Paragraph::new(format!(
        "y/n/a/d · lines {first}-{last}/{} · ↑↓ PgUp/PgDn",
        line_count.max(1)
      ))
      .style(Style::default().fg(APPROVAL_AMBER)),
      sections[1],
    );
  }
  max_scroll
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
  let vertical = Layout::vertical([
    Constraint::Percentage((100 - height_percent) / 2),
    Constraint::Percentage(height_percent),
    Constraint::Percentage((100 - height_percent) / 2),
  ])
  .split(area);
  Layout::horizontal([
    Constraint::Percentage((100 - width_percent) / 2),
    Constraint::Percentage(width_percent),
    Constraint::Percentage((100 - width_percent) / 2),
  ])
  .split(vertical[1])[1]
}
