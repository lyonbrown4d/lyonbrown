use ratatui::{
  Frame,
  layout::{Constraint, Layout, Margin, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{model::App, theme::Theme, view_model::ViewModel};

pub fn render(frame: &mut Frame, app: &App, theme: Theme) {
  render_model(frame, &ViewModel::new(app, theme), theme);
}

/// Rendering consumes presentation data only; it never dispatches daemon actions.
pub fn render_model(frame: &mut Frame, model: &ViewModel<'_>, theme: Theme) {
  let area = frame.area();
  let base = Style::default().fg(theme.text).bg(theme.background);
  frame.render_widget(Block::default().style(base), area);
  if area.is_empty() {
    return;
  }
  let compact = area.width < 70 || area.height < 16;
  let inset = if area.width >= 70 { 2 } else { 0 };
  let area = area.inner(Margin::new(inset, 0));
  let approval_height = model
    .approval
    .as_ref()
    .map_or(0, |approval| {
      if compact {
        4
      } else {
        (approval.description.lines().count() as u16 + 4).clamp(6, 10)
      }
    })
    .min(area.height.saturating_sub(5));
  let composer_height = if compact { 3 } else { 4 };
  let rows = Layout::vertical([
    Constraint::Length(if compact { 2 } else { 3 }),
    Constraint::Min(0),
    Constraint::Length(approval_height),
    Constraint::Length(composer_height),
    Constraint::Length(if compact { 0 } else { 2 }),
  ])
  .split(area);
  header(frame, model, rows[0], theme, compact);
  body(frame, model, rows[1], theme);
  if let Some(approval) = &model.approval {
    let block = Block::default()
      .borders(Borders::TOP | Borders::BOTTOM)
      .border_style(Style::default().fg(theme.warning));
    let inner = block.inner(rows[2]);
    frame.render_widget(block, rows[2]);
    let parts = Layout::vertical([
      Constraint::Length(1),
      Constraint::Min(0),
      Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
      Paragraph::new(Line::styled(
        format!("! 需要批准 · {} 项待处理", approval.pending),
        Style::default()
          .fg(theme.warning)
          .add_modifier(Modifier::BOLD),
      )),
      parts[0],
    );
    if !compact {
      frame.render_widget(
        Paragraph::new(approval.description.as_str()).wrap(Wrap { trim: false }),
        parts[1],
      );
    }
    frame.render_widget(
      Paragraph::new(Line::styled(
        if compact {
          "Ctrl+Y 批准一次  Ctrl+E 拒绝"
        } else {
          "Ctrl+Y 批准一次    Ctrl+E 拒绝    F5 完整内容"
        },
        Style::default().fg(theme.accent),
      )),
      parts[2],
    );
  }
  composer(frame, model, rows[3], theme, compact);
  if !compact {
    let execution = if model.active {
      "Ctrl+X 取消执行 · "
    } else {
      ""
    };
    let hint = format!("{execution}F1 帮助 · F2 会话 · F3 任务 · F4 计算机");
    frame.render_widget(
      Paragraph::new(vec![
        Line::from(hint),
        Line::from(if model.notice.is_empty() {
          "任务由 agentd 持续运行"
        } else {
          &model.notice
        }),
      ])
      .style(Style::default().fg(theme.muted))
      .wrap(Wrap { trim: false }),
      rows[4],
    );
  }
}

fn header(frame: &mut Frame, model: &ViewModel<'_>, area: Rect, theme: Theme, compact: bool) {
  let mut heading = vec![Span::styled(
    "Lyonbrown",
    Style::default()
      .fg(theme.accent)
      .add_modifier(Modifier::BOLD),
  )];
  if area.width >= 45 {
    heading.push(Span::raw(format!("  /  {}", model.title)));
  }
  let status = Style::default().fg(if model.connected {
    theme.muted
  } else {
    theme.warning
  });
  if !compact {
    let block = Block::default()
      .borders(Borders::BOTTOM)
      .border_style(Style::default().fg(theme.muted));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let top = Rect::new(inner.x, inner.y, inner.width, inner.height.min(1));
    let columns = Layout::horizontal([
      Constraint::Min(9),
      Constraint::Length(Line::from(model.connection).width() as u16),
    ])
    .split(top);
    frame.render_widget(Paragraph::new(Line::from(heading)), columns[0]);
    frame.render_widget(Paragraph::new(model.connection).style(status), columns[1]);
    return;
  }
  let lines = vec![Line::from(heading), Line::styled(model.connection, status)];
  frame.render_widget(
    Paragraph::new(lines).block(
      Block::default()
        .borders(if compact {
          Borders::NONE
        } else {
          Borders::BOTTOM
        })
        .border_style(Style::default().fg(theme.muted)),
    ),
    area,
  );
}

fn body(frame: &mut Frame, model: &ViewModel<'_>, area: Rect, theme: Theme) {
  if let Some((title, content)) = &model.context {
    if let Some(task_list) = &model.task_list {
      if area.width >= 95 {
        let columns = Layout::horizontal([Constraint::Min(30), Constraint::Length(34)]).split(area);
        transcript(frame, model, columns[0], theme);
        crate::context_view::render_task_list(frame, title, task_list, columns[1], theme);
      } else {
        crate::context_view::render_task_list(frame, title, task_list, area, theme);
      }
      return;
    }
    if area.width >= 95 {
      let columns = Layout::horizontal([Constraint::Min(30), Constraint::Length(34)]).split(area);
      transcript(frame, model, columns[0], theme);
      crate::context_view::render_text(
        frame,
        title,
        content,
        model.context_scroll,
        columns[1],
        theme,
      );
    } else {
      crate::context_view::render_text(frame, title, content, model.context_scroll, area, theme);
    }
  } else {
    transcript(frame, model, area, theme);
  }
}

fn transcript(frame: &mut Frame, model: &ViewModel<'_>, area: Rect, theme: Theme) {
  let compact = area.width < 66 || area.height < 5;
  let mut text = if compact && model.scroll == 0 {
    model.compact_transcript.clone()
  } else {
    model.transcript.clone()
  };
  for line in &mut text.lines {
    // Presentation markers are owned UI text, never inferred from tool content.
    if matches!(
      line.to_string().as_str(),
      "❯ 你" | "● Lyonbrown" | "● Lyonbrown · 正在回复"
    ) {
      line.style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    }
  }
  let paragraph = Paragraph::new(text)
    .style(Style::default().fg(theme.text))
    .wrap(Wrap { trim: false });
  let end = paragraph
    .line_count(area.width)
    .saturating_sub(usize::from(area.height));
  let scroll = if compact && model.scroll == 0 {
    0
  } else {
    end.saturating_sub(usize::from(model.scroll))
  }
  .min(usize::from(u16::MAX)) as u16;
  frame.render_widget(paragraph.scroll((scroll, 0)), area);
}

fn composer(frame: &mut Frame, model: &ViewModel<'_>, area: Rect, theme: Theme, compact: bool) {
  let title = if model.submitting {
    " 正在发送 · 草稿保留至确认 "
  } else if compact {
    " Enter 发送 · F1 帮助 "
  } else {
    " Enter 发送 · Alt/Shift+Enter 换行 "
  };
  let block = Block::default()
    .title(title)
    .borders(Borders::TOP | Borders::BOTTOM)
    .border_style(Style::default().fg(theme.muted));
  let inner = block.inner(area);
  frame.render_widget(block, area);
  let columns = Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).split(inner);
  frame.render_widget(
    Paragraph::new("❯").style(Style::default().fg(theme.accent)),
    columns[0],
  );
  model.composer.render(frame, columns[1], theme);
}
