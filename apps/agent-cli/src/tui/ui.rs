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
    render_with_approval(frame, state, None);
}

pub(crate) fn render_with_approval(
    frame: &mut Frame<'_>,
    state: &TuiState,
    approval: Option<&ApprovalRequest>,
) {
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
        render_approval(frame, request);
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
    let lines = state.transcript().iter().flat_map(|entry| {
        let (label, color) = match entry.speaker {
            Speaker::User => ("YOU", APPROVAL_AMBER),
            Speaker::Assistant => ("AGENT", SIGNAL_CYAN),
            Speaker::System => ("SYSTEM", APPROVAL_AMBER),
        };
        [
            Line::from(Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(entry.text.clone()),
            Line::default(),
        ]
    });
    frame.render_widget(
        Paragraph::new(Text::from(lines.collect::<Vec<_>>()))
            .block(panel(" CONVERSATION "))
            .wrap(Wrap { trim: false }),
        area,
    );
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

fn render_approval(frame: &mut Frame<'_>, request: &ApprovalRequest) {
    let area = centered_rect(76, 70, frame.area());
    frame.render_widget(Clear, area);
    let text = format!(
        "tool: {}\nrisk: {:?}\narguments:\n{}\nsecret references: {}\n\n[y] allow  [n] deny  [a] session  [d] deny all",
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
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
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
