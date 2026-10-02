use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Clear, Paragraph},
};

use super::{
  CommandId, Locale, OverlayState, Theme, TuiState,
  activity_overlay::render_activity,
  command_palette::{CommandSpec, command_catalog, matching_commands},
  locale::{PaletteMessages, shortcut_line_count},
  ui::panel,
};

pub(super) fn render_overlay(
  frame: &mut Frame<'_>,
  state: &TuiState,
  theme: Theme,
  locale: Locale,
) -> u16 {
  let messages = locale.messages().palette;
  match state.overlay() {
    OverlayState::Closed => 0,
    OverlayState::Palette { query, selected } => {
      render_palette(frame, query, *selected, theme, messages);
      0
    }
    OverlayState::Help { scroll } => {
      render_help(frame, *scroll, theme, messages);
      0
    }
    OverlayState::Activity(inspector) => render_activity(frame, state, inspector, theme, locale),
  }
}

fn render_palette(
  frame: &mut Frame<'_>,
  query: &str,
  selected: usize,
  theme: Theme,
  messages: PaletteMessages,
) {
  let area = overlay_area(frame.area());
  frame.render_widget(Clear, area);
  let block = panel(messages.title, theme, true);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if inner.is_empty() {
    return;
  }
  let rows = Layout::vertical([
    Constraint::Length(1),
    Constraint::Min(1),
    Constraint::Length(u16::from(inner.height > 2)),
  ])
  .split(inner);
  frame.render_widget(
    Paragraph::new(format!("{}: {query}", messages.search))
      .style(Style::default().fg(theme.text_secondary)),
    rows[0],
  );
  let commands = matching_commands(query);
  if commands.is_empty() {
    frame.render_widget(
      Paragraph::new(messages.no_matches).style(Style::default().fg(theme.text_faint)),
      rows[1],
    );
  } else {
    let lines = visible_command_lines(&commands, selected, rows[1].height, theme, messages);
    frame.render_widget(Paragraph::new(lines), rows[1]);
  }
  if rows[2].height > 0 {
    frame.render_widget(
      Paragraph::new(messages.select_hint).style(Style::default().fg(theme.text_faint)),
      rows[2],
    );
  }
}

fn visible_command_lines(
  commands: &[&CommandSpec],
  selected: usize,
  height: u16,
  theme: Theme,
  messages: PaletteMessages,
) -> Vec<Line<'static>> {
  let height = usize::from(height);
  let start = selected.saturating_sub(height.saturating_sub(1));
  commands
    .iter()
    .enumerate()
    .skip(start)
    .take(height)
    .map(|(index, command)| command_line(command, index == selected, theme, messages))
    .collect()
}

fn command_line(
  command: &CommandSpec,
  selected: bool,
  theme: Theme,
  messages: PaletteMessages,
) -> Line<'static> {
  let enabled = command.implemented && command.id != CommandId::Queue;
  let marker = if selected { "› " } else { "  " };
  let color = if enabled {
    theme.text_primary
  } else {
    theme.text_faint
  };
  let suffix = if enabled {
    command_label(command.id, messages)
  } else {
    disabled_label(command.id, messages)
  };
  Line::from(vec![
    Span::styled(marker.to_owned(), Style::default().fg(theme.accent_primary)),
    Span::styled(
      command.display.to_owned(),
      Style::default().fg(color).add_modifier(if selected {
        Modifier::BOLD
      } else {
        Modifier::empty()
      }),
    ),
    Span::styled(format!(" · {suffix}"), Style::default().fg(color)),
  ])
}

fn command_label(id: CommandId, messages: PaletteMessages) -> &'static str {
  match id {
    CommandId::Model => messages.model,
    CommandId::Theme => messages.theme,
    CommandId::Language => messages.language,
    CommandId::Sessions => messages.sessions,
    CommandId::New => messages.new_session,
    CommandId::Resume => messages.resume,
    CommandId::Status => messages.status,
    CommandId::Queue => messages.queue,
    CommandId::Help => messages.help_command,
    CommandId::Quit => messages.quit,
  }
}

fn disabled_label(id: CommandId, messages: PaletteMessages) -> &'static str {
  if id == CommandId::Queue {
    return messages.queue_disabled;
  }
  messages.command_disabled
}

fn render_help(frame: &mut Frame<'_>, scroll: u16, theme: Theme, messages: PaletteMessages) {
  let area = overlay_area(frame.area());
  frame.render_widget(Clear, area);
  let block = panel(messages.help_title, theme, true);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  let lines = help_lines(messages);
  let scroll = scroll.min(help_max_scroll(frame.area().height));
  frame.render_widget(
    Paragraph::new(lines).scroll((scroll, 0)).style(
      Style::default()
        .fg(theme.text_primary)
        .bg(theme.surface_raised),
    ),
    inner,
  );
}

fn help_lines(messages: PaletteMessages) -> Vec<Line<'static>> {
  let mut lines = messages
    .shortcut_lines
    .iter()
    .copied()
    .map(Line::from)
    .collect::<Vec<_>>();
  lines.push(Line::default());
  lines.extend(command_catalog().iter().map(|command| {
    let availability = if !command.implemented {
      messages.disabled
    } else if command.daemon_backed {
      messages.daemon_available
    } else {
      messages.local_available
    };
    Line::from(format!(
      "{} · {} · {availability}",
      command.display,
      command_label(command.id, messages)
    ))
  }));
  lines
}

pub(crate) fn help_max_scroll(viewport_height: u16) -> u16 {
  let overlay_height = if viewport_height < 14 {
    viewport_height
  } else {
    viewport_height.saturating_mul(72) / 100
  };
  let body_height = overlay_height.saturating_sub(2);
  let line_count = shortcut_line_count()
    .saturating_add(1)
    .saturating_add(command_catalog().len() as u16);
  line_count.saturating_sub(body_height)
}

fn overlay_area(area: Rect) -> Rect {
  if area.width < 54 || area.height < 14 {
    area
  } else {
    let width = area.width.saturating_mul(76) / 100;
    let height = area.height.saturating_mul(72) / 100;
    Rect::new(
      area.x + area.width.saturating_sub(width) / 2,
      area.y + area.height.saturating_sub(height) / 2,
      width,
      height,
    )
  }
}
