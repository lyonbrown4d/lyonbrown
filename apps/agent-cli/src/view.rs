use ratatui::{
  Frame,
  layout::{Constraint, Layout},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::{
  model::{App, Connection},
  theme::Theme,
};

pub fn render(frame: &mut Frame, app: &App, theme: Theme) {
  if frame.area().width < 70 || frame.area().height < 15 {
    compact(frame, app, theme);
    return;
  }
  let rows = Layout::vertical([
    Constraint::Length(3),
    Constraint::Min(5),
    Constraint::Length(4),
    Constraint::Length(2),
  ])
  .split(frame.area());
  let (status, color) = match &app.connection {
    Connection::Connected => ("Connected".to_owned(), theme.accent),
    Connection::Connecting => ("Connecting…".to_owned(), theme.muted),
    Connection::Disconnected(reason) => (format!("Disconnected · {reason}"), theme.warning),
  };
  frame.render_widget(
    Paragraph::new(vec![
      Line::from(vec![
        Span::styled(
          " Lyonbrown ",
          Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status, Style::default().fg(color)),
      ]),
      Line::from(app.computer.summary()),
    ])
    .wrap(Wrap { trim: false }),
    rows[0],
  );
  let columns = Layout::horizontal([
    Constraint::Length(24),
    Constraint::Min(20),
    Constraint::Length(27),
  ])
  .split(rows[1]);
  let sessions = app
    .sessions
    .iter()
    .enumerate()
    .map(|(index, session)| {
      ListItem::new(format!(
        "{} {}",
        if index == app.selected { "›" } else { " " },
        session.title
      ))
    })
    .collect::<Vec<_>>();
  frame.render_widget(
    List::new(sessions).block(panel("Sessions", theme)),
    columns[0],
  );
  let mut conversation = Vec::new();
  if app.messages.is_empty() {
    conversation.push(Line::styled(
      "Start a conversation with your agent.",
      Style::default().fg(theme.muted),
    ));
    conversation.push(Line::from(
      "Tasks keep running in agentd when this terminal closes.",
    ));
  }
  for message in &app.messages {
    conversation.push(Line::styled(
      &message.role,
      Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD),
    ));
    conversation.extend(
      message
        .content
        .lines()
        .map(|line| Line::from(line.to_owned())),
    );
    conversation.push(Line::from(""));
  }
  if let Some((_, text)) = &app.streaming {
    conversation.push(Line::styled("Agent", Style::default().fg(theme.accent)));
    conversation.extend(text.lines().map(|line| Line::from(line.to_owned())));
  }
  frame.render_widget(
    Paragraph::new(conversation)
      .wrap(Wrap { trim: false })
      .scroll((app.scroll, 0))
      .block(panel("Conversation", theme)),
    columns[1],
  );
  let side = Layout::vertical([
    Constraint::Percentage(35),
    Constraint::Percentage(35),
    Constraint::Percentage(30),
  ])
  .split(columns[2]);
  let tasks = if app.tasks.is_empty() {
    "No tasks".into()
  } else {
    app.tasks.join("\n\n")
  };
  frame.render_widget(
    Paragraph::new(tasks)
      .wrap(Wrap { trim: false })
      .block(panel("Tasks", theme)),
    side[0],
  );
  let approvals = if app.approvals.is_empty() {
    "No pending approvals".into()
  } else {
    app
      .approvals
      .iter()
      .map(|approval| format!("{}\n{}", approval.id, approval.description))
      .collect::<Vec<_>>()
      .join("\n\n")
  };
  frame.render_widget(
    Paragraph::new(approvals)
      .wrap(Wrap { trim: false })
      .block(panel("Approvals", theme)),
    side[1],
  );
  frame.render_widget(
    Paragraph::new(if app.computer_detail.is_empty() {
      "/computer targets · attach window ID|display ID · detach · status"
    } else {
      &app.computer_detail
    })
    .wrap(Wrap { trim: false })
    .scroll((app.computer_scroll, 0))
    .block(panel("Computer · Alt+PgUp/PgDn", theme)),
    side[2],
  );
  composer(frame, app, rows[2], theme);
  frame.render_widget(
    Paragraph::new(app.notice.as_str())
      .style(Style::default().fg(theme.muted))
      .wrap(Wrap { trim: false }),
    rows[3],
  );
}

fn compact(frame: &mut Frame, app: &App, theme: Theme) {
  let connection = match &app.connection {
    Connection::Connected => "Connected".to_owned(),
    Connection::Connecting => "Connecting…".to_owned(),
    Connection::Disconnected(reason) => format!("Disconnected · {reason}"),
  };
  let mut lines = vec![
    Line::styled(
      "Lyonbrown",
      Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD),
    ),
    Line::from(connection),
    Line::from(app.computer.summary()),
  ];
  lines.extend(
    app
      .computer_detail
      .lines()
      .skip(usize::from(app.computer_scroll))
      .take(3)
      .map(|line| Line::from(line.to_owned())),
  );
  if let Some(session) = app.selected_session() {
    lines.push(Line::from(session.title.clone()));
  }
  if let Some((_, text)) = &app.streaming {
    lines.extend(text.lines().map(|line| Line::from(line.to_owned())));
  } else if let Some(message) = app.messages.last() {
    lines.extend(
      message
        .content
        .lines()
        .map(|line| Line::from(line.to_owned())),
    );
  }
  lines.push(Line::from(app.notice.clone()));
  let rows = Layout::vertical([
    Constraint::Min(1),
    Constraint::Length(frame.area().height.min(3)),
  ])
  .split(frame.area());
  frame.render_widget(
    Paragraph::new(lines)
      .style(Style::default().fg(theme.text))
      .wrap(Wrap { trim: false }),
    rows[0],
  );
  composer(frame, app, rows[1], theme);
}

fn composer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect, theme: Theme) {
  let block = panel("Message · Enter send · Alt/Shift+Enter newline", theme);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  app.input.render(frame, inner);
}

fn panel(title: &str, theme: Theme) -> Block<'_> {
  Block::default()
    .title(title)
    .borders(Borders::ALL)
    .border_style(Style::default().fg(theme.muted))
    .style(Style::default().fg(theme.text))
}
