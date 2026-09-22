use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::{ApprovalRequest, Speaker, TuiState};

const MIDNIGHT_SLATE: Color = Color::Rgb(0x10, 0x18, 0x20);
const DEEP_NAVY: Color = Color::Rgb(0x17, 0x2A, 0x3A);
const SIGNAL_CYAN: Color = Color::Rgb(0x36, 0xC5, 0xD0);
const APPROVAL_AMBER: Color = Color::Rgb(0xF2, 0xB8, 0x4B);
const PAPER: Color = Color::Rgb(0xE8, 0xEE, 0xF2);
const MUTED_STEEL: Color = Color::Rgb(0x78, 0x90, 0x9C);

pub fn render(frame: &mut Frame<'_>, state: &TuiState) {
    let _ = render_with_approval(frame, state, None, 0);
}

#[doc(hidden)]
pub fn render_with_approval(
    frame: &mut Frame<'_>,
    state: &TuiState,
    approval: Option<&ApprovalRequest>,
    approval_scroll: u16,
) -> u16 {
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

    if frame.area().width >= 90 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(35), Constraint::Length(30)])
            .split(rows[0]);
        render_conversation(frame, columns[0], state);
        render_runtime(frame, columns[1], state);
    } else {
        render_conversation(frame, rows[0], state);
    }
    render_composer(frame, rows[1], state);
    if let Some(request) = approval {
        render_approval(frame, request, approval_scroll)
    } else {
        0
    }
}

fn panel(title: &'static str) -> Block<'static> {
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

fn render_conversation(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
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
    let paragraph = Paragraph::new(text)
        .block(panel(" CONVERSATION "))
        .wrap(Wrap { trim: false });
    let viewport_height = usize::from(area.height.saturating_sub(2));
    let scroll = rendered_lines
        .saturating_sub(viewport_height)
        .min(usize::from(u16::MAX)) as u16;
    frame.render_widget(paragraph.scroll((scroll, 0)), area);
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

fn render_composer(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let title = if state.busy { " BUSY " } else { " Message " };
    let content = if state.input.is_empty() {
        "Enter send · Esc/Ctrl-C quit"
    } else {
        &state.input
    };
    let color = if state.input.is_empty() {
        MUTED_STEEL
    } else {
        PAPER
    };
    frame.render_widget(
        Paragraph::new(Span::styled(content, Style::default().fg(color))).block(panel(title)),
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
