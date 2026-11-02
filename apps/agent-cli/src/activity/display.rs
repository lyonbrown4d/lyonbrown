use ratatui::{
  style::{Modifier, Style},
  text::{Line, Span, Text},
};

use crate::theme::Theme;

use super::{ActivityItem, ActivityKind, ActivityState, ActivityStatus, values::short};

impl ActivityState {
  pub fn summary(&self) -> String {
    let progress = self
      .items
      .iter()
      .rev()
      .find(|item| item.status == ActivityStatus::Running)
      .or_else(|| {
        self
          .items
          .iter()
          .rev()
          .find(|item| item.kind == ActivityKind::Run)
      })
      .or_else(|| self.items.last())
      .map(|item| {
        format!(
          "{}{}",
          item.title,
          if item.target.is_empty() {
            String::new()
          } else {
            format!(" · {}", item.target)
          }
        )
      })
      .unwrap_or_else(|| "暂无执行活动".into());
    let context = self
      .items
      .iter()
      .rev()
      .find_map(|item| item.model.as_ref().and_then(|model| model.context_window))
      .map(|tokens| format!(" · 上下文上限 {tokens}"))
      .unwrap_or_default();
    let progress = format!("{progress}{context}");
    let usage = self.token_usage();
    if usage.total == 0 {
      progress
    } else {
      format!(
        "{progress} · Token {}（输入 {} / 输出 {}）",
        usage.total, usage.input, usage.output
      )
    }
  }

  /// The first rendered line of the selected item, for a wrapping Paragraph's
  /// scroll calculation. No terminal height or execution handle is retained.
  pub fn selected_line(&self) -> usize {
    // One header plus a blank separator.
    2 + self
      .items
      .iter()
      .take(self.selected)
      .map(item_height)
      .sum::<usize>()
  }

  pub fn text(&self, theme: Theme) -> Text<'static> {
    let mut lines = vec![
      Line::styled(
        self.summary(),
        Style::default()
          .fg(theme.accent)
          .add_modifier(Modifier::BOLD),
      ),
      Line::default(),
    ];
    if self.items.is_empty() {
      lines.push(Line::styled(
        "工具、模型阶段和上下文整理将在执行后显示",
        Style::default().fg(theme.muted),
      ));
    }
    for (index, item) in self.items.iter().enumerate() {
      let selected = index == self.selected;
      let color = match item.status {
        ActivityStatus::Failed | ActivityStatus::Uncertain => theme.warning,
        ActivityStatus::Running => theme.accent,
        _ => theme.text,
      };
      let fold = if item.expanded { "▾" } else { "▸" };
      let kind = match item.kind {
        ActivityKind::Run => "模型",
        ActivityKind::Tool => "工具",
        ActivityKind::Compaction => "上下文",
      };
      let elapsed = item
        .elapsed_ms()
        .map(|ms| format!(" · {}.{:01} 秒", ms / 1000, ms % 1000 / 100))
        .unwrap_or_default();
      lines.push(Line::from(vec![
        Span::styled(
          if selected { "› " } else { "  " },
          Style::default().fg(theme.accent),
        ),
        Span::styled(
          format!(
            "{fold} {kind} · {} · {}{elapsed}",
            item.title,
            label(item.status)
          ),
          Style::default().fg(color).add_modifier(if selected {
            Modifier::BOLD
          } else {
            Modifier::empty()
          }),
        ),
      ]));
      if !item.target.is_empty() {
        lines.push(Line::styled(
          format!("    {}", item.target),
          Style::default().fg(theme.muted),
        ));
      }
      if item.expanded {
        for (label, value) in [("参数", &item.arguments), ("结果", &item.result)] {
          if !value.is_empty() {
            lines.push(Line::styled(
              format!("    {label}"),
              Style::default().fg(theme.accent),
            ));
            lines.extend(
              value
                .lines()
                .map(|line| Line::styled(format!("      {line}"), Style::default().fg(theme.text))),
            );
          }
        }
      } else if !item.result.is_empty() {
        lines.push(Line::styled(
          format!("    {}", short(&item.result, 160)),
          Style::default().fg(theme.muted),
        ));
      }
      lines.push(Line::default());
    }
    Text::from(lines)
  }
}

fn item_height(item: &ActivityItem) -> usize {
  let target = usize::from(!item.target.is_empty());
  let details = if item.expanded {
    [&item.arguments, &item.result]
      .iter()
      .filter(|value| !value.is_empty())
      .map(|value| 1 + value.lines().count())
      .sum()
  } else {
    usize::from(!item.result.is_empty())
  };
  2 + target + details
}

fn label(status: ActivityStatus) -> &'static str {
  match status {
    ActivityStatus::Running => "进行中",
    ActivityStatus::Succeeded => "成功",
    ActivityStatus::Failed => "失败",
    ActivityStatus::Cancelled => "已取消",
    ActivityStatus::Uncertain => "结果待确认",
  }
}
