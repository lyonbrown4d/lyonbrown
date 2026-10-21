//! Pure presentation projection. Daemon actions and authority stay in input/update.
use ratatui::text::{Line, Text};

use crate::{
  composer::Composer,
  model::{App, Connection, ContextPanel},
  theme::Theme,
};

pub struct ApprovalView {
  pub description: String,
  pub pending: usize,
}

pub struct ViewModel<'a> {
  pub connection: &'static str,
  pub connected: bool,
  pub title: String,
  pub transcript: Text<'static>,
  pub compact_transcript: Text<'static>,
  pub approval: Option<ApprovalView>,
  pub context: Option<(&'static str, Text<'static>)>,
  pub composer: &'a Composer,
  pub submitting: bool,
  pub active: bool,
  pub notice: String,
  pub scroll: u16,
  pub context_scroll: u16,
}

impl<'a> ViewModel<'a> {
  pub fn new(app: &'a App, theme: Theme) -> Self {
    let connection = match app.connection {
      Connection::Connected => "agentd 已连接 · Connected",
      Connection::Connecting => "agentd 连接中 · Connecting…",
      Connection::Disconnected(_) => "agentd 已断开 · Disconnected · Ctrl+R 重连",
    };
    let mut lines = Vec::new();
    if app.messages.is_empty() && app.streaming.is_none() {
      lines.extend([
        Line::from(""),
        Line::from("从一个具体问题开始。"),
        Line::from(""),
        Line::from("会话与任务保存在 agentd。"),
        Line::from("关闭终端不会取消正在进行的任务。"),
        Line::from(""),
        Line::from("可以这样开始"),
        Line::from("❯ 先读取 Cargo.toml，帮我梳理工作区结构。"),
      ]);
    }
    for message in &app.messages {
      let role = match message.role.as_str() {
        "You" => "❯ 你",
        "Error" => "! 错误",
        _ => "● Lyonbrown",
      };
      lines.push(Line::from(role));
      let content = safe_text(&message.content);
      if message.role == "Agent" {
        lines.extend(crate::markdown::render(&content, theme).lines);
      } else {
        lines.extend(content.lines().map(|line| Line::from(line.to_owned())));
      }
      lines.push(Line::from(""));
    }
    if let Some((_, content)) = &app.streaming {
      lines.push(Line::from("● Lyonbrown · 正在回复"));
      lines.extend(crate::markdown::render(&safe_text(content), theme).lines);
    }
    let context = match app.panel {
      ContextPanel::Closed => None,
      ContextPanel::Help => Some((
        "快捷键 · F1 关闭",
        Text::from(
          "Enter 发送\nAlt/Shift+Enter 换行\nPgUp/PgDn 滚动对话或面板\nCtrl+N 新建会话\nTab 切换会话\nCtrl+R 重连\nCtrl+X 取消执行\nCtrl+Y 批准一次\nCtrl+E 拒绝\nCtrl+C / Ctrl+D 退出\n\nF2 会话 · F3 任务 · F4 计算机\nF5 完整审批内容\nEsc 关闭面板\n\n/computer targets\n/computer attach window ID\n/computer attach display ID\n/computer detach\n/computer status\nAlt+PgUp/PgDn 滚动计算机详情",
        ),
      )),
      ContextPanel::Sessions => Some((
        "会话 · Tab 切换",
        Text::from(
          app
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
              Line::from(format!(
                "{} {}",
                if index == app.selected { "›" } else { " " },
                safe_text(&session.title)
              ))
            })
            .collect::<Vec<_>>(),
        ),
      )),
      ContextPanel::Tasks => Some((
        "任务 · Ctrl+X 取消执行",
        Text::from(if app.tasks.is_empty() {
          "当前会话没有任务".to_owned()
        } else {
          // Run identifiers remain in the model, not in the presentation.
          app
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| {
              let status = task.lines().last().unwrap_or("未知");
              format!("任务 {} · {}", index + 1, safe_text(status))
            })
            .collect::<Vec<_>>()
            .join("\n")
        }),
      )),
      ContextPanel::Approval => Some((
        "审批详情 · PgUp/PgDn 滚动",
        Text::from(
          app
            .approvals
            .first()
            .map(|approval| safe_text(&approval.description))
            .unwrap_or_else(|| "没有待处理的审批".into()),
        ),
      )),
      ContextPanel::Computer => Some((
        "计算机 · Alt+PgUp/PgDn",
        Text::from(format!(
          "{}\n\n{}\n\n/computer targets · attach · detach · status",
          computer_status(
            &app.computer,
            app.selected_session().map(|session| session.id.as_str())
          ),
          safe_text(&app.computer_detail)
        )),
      )),
    };
    let compact_transcript = if let Some((_, content)) = &app.streaming {
      crate::markdown::render(&safe_text(content), theme)
    } else if let Some(message) = app.messages.last() {
      crate::markdown::render(&safe_text(&message.content), theme)
    } else {
      Text::from("从一个具体问题开始。\nF1 帮助 · Ctrl+N 新建会话")
    };
    Self {
      connection,
      connected: app.connection == Connection::Connected,
      title: app
        .selected_session()
        .map(|session| safe_text(&session.title))
        .unwrap_or_else(|| "新会话".into()),
      transcript: Text::from(lines),
      compact_transcript,
      approval: app.approvals.first().map(|approval| ApprovalView {
        description: safe_text(&approval.description),
        pending: app.approvals.len(),
      }),
      context,
      composer: &app.input,
      submitting: app.submitting,
      active: app.active_run.is_some(),
      notice: safe_text(&app.notice),
      scroll: app.scroll,
      context_scroll: app.computer_scroll,
    }
  }
}

/// Never forward terminal controls or bidi overrides from model/tool/native output.
pub fn safe_text(value: &str) -> String {
  value
    .chars()
    .filter(|ch| {
      (!ch.is_control() || matches!(ch, '\n' | '\t'))
        && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
    .collect()
}

fn computer_status(status: &crate::computer::ComputerStatus, selected: Option<&str>) -> String {
  use crate::computer::{ComputerStatus, scope_text};
  match status {
    ComputerStatus::Unavailable => "计算机控制：此构建不可用".into(),
    ComputerStatus::Detached => "计算机控制：未绑定".into(),
    ComputerStatus::Attaching { scope, session } => {
      format!(
        "计算机控制：正在绑定 {} · {}",
        scope_text(scope),
        owner_label(session, selected)
      )
    }
    ComputerStatus::Attached { scope, session } => format!(
      "计算机控制：已绑定 {} · {}",
      scope_text(scope),
      owner_label(session, selected)
    ),
    ComputerStatus::Stopped { reason, .. } => format!("计算机控制：{reason}"),
  }
}

fn owner_label(owner: &str, selected: Option<&str>) -> &'static str {
  if selected == Some(owner) {
    "属于当前会话"
  } else {
    "属于另一会话；切换不会转移控制权"
  }
}
