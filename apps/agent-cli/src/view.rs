use ratatui::{
  Frame,
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
  diff::model::DiffView,
  layout::PaletteLayout,
  model::App,
  theme::Theme,
  view_model::{CommandPaletteView, ViewModel},
};

pub fn render(frame: &mut Frame, app: &App, theme: Theme) {
  render_model(frame, &ViewModel::new(app, theme), theme);
}

/// Rendering consumes presentation data only; it never dispatches daemon actions.
pub fn render_model(frame: &mut Frame, model: &ViewModel<'_>, theme: Theme) {
  let area = frame.area();
  if let Some(view) = model.diff {
    diff_screen(frame, view, area, theme);
    return;
  }
  let base = Style::default().fg(theme.text).bg(theme.background);
  frame.render_widget(Block::default().style(base), area);
  if area.is_empty() {
    return;
  }
  let layout = crate::layout::AppLayout::new(
    area,
    model
      .approval
      .as_ref()
      .map(|approval| approval.description.lines().count()),
  );
  let compact = layout.compact;
  header(frame, model, layout.header, theme, compact);
  body(frame, model, layout.body, theme);
  if let Some(approval) = &model.approval {
    let block = Block::default()
      .borders(Borders::TOP | Borders::BOTTOM)
      .border_style(Style::default().fg(theme.warning));
    let inner = block.inner(layout.approval);
    frame.render_widget(block, layout.approval);
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
  composer(frame, model, layout.composer, theme, compact);
  if let Some(palette) = &model.command_palette
    && !palette.items.is_empty()
  {
    command_palette(frame, palette, layout.palette(palette.items.len()), theme);
  }
  if let Some(picker) = model.picker {
    let popup = picker_overlay(&layout, picker.visible_rows());
    crate::picker::render(frame, popup, picker, theme);
  }
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
      layout.footer,
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
  let layout = crate::layout::BodyLayout::new(area, model.context.is_some());
  if let Some(transcript_area) = layout.transcript {
    transcript(frame, model, transcript_area, theme);
  }
  if let (Some(context_area), Some((title, content))) = (layout.context, &model.context) {
    if let Some(task_list) = &model.task_list {
      crate::context_view::render_task_list(frame, title, task_list, context_area, theme);
    } else {
      crate::context_view::render_text(
        frame,
        title,
        content,
        model.context_scroll,
        context_area,
        theme,
      );
    }
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

/// Completion floats above the composer; hidden matches are reported instead of scrolled away.
fn command_palette(
  frame: &mut Frame,
  palette: &CommandPaletteView,
  popup: PaletteLayout,
  theme: Theme,
) {
  frame.render_widget(Clear, popup.area);
  let block = Block::default()
    .title(" 命令 · ↑↓ 选择 · Tab 补全 · Esc 收起 ")
    .borders(Borders::ALL)
    .border_style(Style::default().fg(theme.accent));
  let inner = block.inner(popup.area);
  frame.render_widget(block, popup.area);
  let hint_visible = popup.has_more && inner.height > 1;
  let list_area = if hint_visible {
    Rect::new(inner.x, inner.y, inner.width, inner.height - 1)
  } else {
    inner
  };
  let start = palette
    .selected
    .saturating_sub(popup.rows.saturating_sub(1));
  let items = palette
    .items
    .iter()
    .skip(start)
    .take(popup.rows)
    .map(|(name, summary)| ListItem::new(format!("{name:<14} {summary}")))
    .collect::<Vec<_>>();
  let mut state = ListState::default();
  state.select(Some(palette.selected.saturating_sub(start)));
  frame.render_stateful_widget(
    List::new(items).highlight_symbol("› ").highlight_style(
      Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD),
    ),
    list_area,
    &mut state,
  );
  if hint_visible {
    frame.render_widget(
      Paragraph::new(Line::styled(
        format!(
          "↓ 还有 {} 条 · 共 {} 条",
          palette.items.len().saturating_sub(popup.rows),
          palette.items.len()
        ),
        Style::default().fg(theme.muted),
      )),
      Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
  }
}

/// Selection overlays sit above the composer, matching the completion popup column.
fn picker_overlay(layout: &crate::layout::AppLayout, rows: usize) -> Rect {
  let chrome = 4;
  let height = (rows.saturating_add(chrome) as u16)
    .min(layout.composer.y.saturating_sub(layout.body.y))
    .max(3);
  Rect::new(
    layout.composer.x.saturating_add(2),
    layout.composer.y.saturating_sub(height),
    layout.composer.width.saturating_sub(2).min(72),
    height,
  )
}

/// The diff owns the whole screen: a title bar, the body, and a key hint line.
fn diff_screen(frame: &mut Frame, view: &DiffView, area: Rect, theme: Theme) {
  let base = Style::default().fg(theme.text).bg(theme.background);
  frame.render_widget(Block::default().style(base), area);
  if area.is_empty() {
    return;
  }
  let [title_area, body_area, status_area] = Layout::vertical([
    Constraint::Length(1),
    Constraint::Min(0),
    Constraint::Length(1),
  ])
  .areas(area);
  let stats = view.document.stats();
  let mode = match view.state.mode {
    crate::diff::model::DiffMode::Unified => "unified",
    crate::diff::model::DiffMode::SideBySide => "side-by-side",
  };
  let path = if view.path.is_empty() {
    "工作区改动".to_owned()
  } else {
    view.path.clone()
  };
  let position = if stats.files == 0 {
    String::new()
  } else {
    format!(
      " · {}/{} 文件",
      view.state.file.min(stats.files - 1) + 1,
      stats.files
    )
  };
  frame.render_widget(
    Paragraph::new(vec![Line::from(vec![
      Span::styled(
        path,
        Style::default()
          .fg(theme.accent)
          .add_modifier(Modifier::BOLD),
      ),
      Span::styled(
        format!("  {mode} · +{} −{}{position}", stats.added, stats.removed),
        Style::default().fg(theme.muted),
      ),
    ])]),
    title_area,
  );
  crate::diff::render::render(frame, body_area, &view.document, &view.state, theme);
  frame.render_widget(
    Paragraph::new(Line::styled(
      "j/k 滚动 · n/p 文件 · Tab 并排 · h/l 横向 · g/G 首尾 · Esc 关闭",
      Style::default().fg(theme.muted),
    )),
    status_area,
  );
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
